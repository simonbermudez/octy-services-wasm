from schedule import Scheduler
from traceback import format_exc
from datetime import datetime as dt
from datetime import timedelta as td
import asyncio
import collections
import datetime
import functools
import random


class CancelJob(object):
    """
    Can be returned from a job to unschedule itself.
    """
    pass

class RobustScheduler(Scheduler):
    """
    An implementation of Scheduler that catches octy jobs that fail, logs their
    exception tracebacks as errors, optionally reschedules the octy jobs for their
    next run time, and keeps going.

    Use this to run octy jobs that may or may not crash without worrying about
    whether other octy jobs will run or if they'll crash the entire script.
    """

    def __init__(self, logger):
        self.jobs = []
        self.logger = logger
        super().__init__()


    async def run_pending(self, *args, **kwargs):
        """Run all jobs that are scheduled to run.

        Please note that it is *intended behavior that run_pending()
        does not run missed jobs*. For example, if you've registered a job
        that should run every minute and you only call run_pending()
        in one hour increments then your job won't be run 60 times in
        between but only once.

		*timeout* can be used to control the maximum number of seconds to wait before
		returning.  *timeout* can be an int or float.  If *timeout* is not specified
		or ``None``, there is no limit to the wait time.

		*return_when* indicates when this function should return.  It must be one of
		the following constants:

		.. tabularcolumns:: |l|L|

		+-----------------------------+----------------------------------------+
		| Constant                    | Description                            |
		+=============================+========================================+
		| :const:`FIRST_COMPLETED`    | The function will return when any      |
		|                             | future finishes or is cancelled.       |
		+-----------------------------+----------------------------------------+
		| :const:`FIRST_EXCEPTION`    | The function will return when any      |
		|                             | future finishes by raising an          |
		|                             | exception.  If no future raises an     |
		|                             | exception then it is equivalent to     |
		|                             | :const:`ALL_COMPLETED`.                |
		+-----------------------------+----------------------------------------+
		| :const:`ALL_COMPLETED`      | The function will return when all      |
		|                             | futures finish or are cancelled.       |
		+-----------------------------+----------------------------------------+
        """


        jobs = [asyncio.create_task(job.run()) for job in self.jobs if job.should_run]
    
        if not jobs:
            return [], []

        await asyncio.sleep(1)  

        return await asyncio.wait(jobs, *args, **kwargs)
    

    async def run_all(self, delay_seconds=0, *args, **kwargs):
        """Run all jobs regardless if they are scheduled to run or not.

		*timeout* can be used to control the maximum number of seconds to wait before
		returning.  *timeout* can be an int or float.  If *timeout* is not specified
		or ``None``, there is no limit to the wait time.

		*return_when* indicates when this function should return.  It must be one of
		the following constants:

		.. tabularcolumns:: |l|L|

		+-----------------------------+----------------------------------------+
		| Constant                    | Description                            |
		+=============================+========================================+
		| :const:`FIRST_COMPLETED`    | The function will return when any      |
		|                             | future finishes or is cancelled.       |
		+-----------------------------+----------------------------------------+
		| :const:`FIRST_EXCEPTION`    | The function will return when any      |
		|                             | future finishes by raising an          |
		|                             | exception.  If no future raises an     |
		|                             | exception then it is equivalent to     |
		|                             | :const:`ALL_COMPLETED`.                |
		+-----------------------------+----------------------------------------+
		| :const:`ALL_COMPLETED`      | The function will return when all      |
		|                             | futures finish or are cancelled.       |
		+-----------------------------+----------------------------------------+
		"""

        if delay_seconds:
            warnings.warn("The `delay_seconds` parameter is deprecated.",
                DeprecationWarning)
        
        jobs = [asyncio.create_task(self._run_job(job)) for job in self.jobs[:]]
        
        if not jobs:
            return [], []

        return await asyncio.wait(jobs, *args, **kwargs)


    def clear(self, tag=None):
        """
        Deletes scheduled jobs marked with the given tag, or all jobs
        if tag is omitted.

        :param tag: An identifier used to identify a subset of
                    jobs to delete
        """
        if tag is None:
            del self.jobs[:]
        else:
            self.jobs[:] = (job for job in self.jobs if tag not in job.tags)

    def cancel_job(self, job):
        """
        Delete a scheduled job.

        :param job: The job to be unscheduled
        """
        try:
            self.jobs.remove(job)
        except ValueError:
            pass

    def every(self, interval=1):
        """
        Schedule a new periodic job.

        :param interval: A quantity of a certain time unit
        :return: An unconfigured :class:`Job <Job>`
        """
        job = Job(interval, self.logger, self)
        return job

    async def _run_job(self, job):
        ret = await job.run()
        if isinstance(ret, CancelJob) or ret is CancelJob:
            self.cancel_job(job)


    @property
    def next_run(self):
        """
        Datetime when the next job should run.

        :return: A :class:`~datetime.datetime` object
        """
        if not self.jobs:
            return None
        return min(self.jobs).next_run

    @property
    def idle_seconds(self):
        """
        :return: Number of seconds until
                 :meth:`next_run <Scheduler.next_run>`.
        """
        return (self.next_run - datetime.datetime.now()).total_seconds()

class Job(object):
    """
    A periodic job as used by :class:`Scheduler`.

    :param interval: A quantity of a certain time unit
    :param scheduler: The :class:`Scheduler <Scheduler>` instance that
                      this job will register itself with once it has
                      been fully configured in :meth:`Job.do()`.

    Every job runs at a given fixed time interval that is defined by:

    * a :meth:`time unit <Job.second>`
    * a quantity of `time units` defined by `interval`

    A job is usually created and returned by :meth:`Scheduler.every`
    method, which also defines its `interval`.
    """
    def __init__(self, interval,logger, scheduler=None):
        self.interval = interval  # pause interval * unit between runs
        self.latest = None  # upper limit to the interval
        self.job_func = None  # the job job_func to run
        self.unit = None  # time units, e.g. 'minutes', 'hours', ...
        self.at_time = None  # optional time at which this job runs
        self.last_run = None  # datetime of the last run
        self.next_run = None  # datetime of the next run
        self.period = None  # timedelta between runs, only valid for
        self.start_day = None  # Specific day of the week to start on
        self.tags = set()  # unique set of tags for the job
        self.scheduler = scheduler  # scheduler to register with
        self.logger = logger
        self.reschedule_on_failure = True
        self.minutes_after_failure = 0
        self.seconds_after_failure = 30
        self.is_retry = False

    def __lt__(self, other):
        """
        PeriodicJobs are sortable based on the scheduled time they
        run next.
        """
        return self.next_run < other.next_run

    def __repr__(self):
        def format_time(t):
            return t.strftime('%Y-%m-%d %H:%M:%S') if t else '[never]'

        timestats = '(last run: %s, next run: %s)' % (
                    format_time(self.last_run), format_time(self.next_run))

        if hasattr(self.job_func, '__name__'):
            job_func_name = self.job_func.__name__
        else:
            job_func_name = repr(self.job_func)
        args = [repr(x) for x in self.job_func.args]
        kwargs = ['%s=%s' % (k, repr(v))
                  for k, v in self.job_func.keywords.items()]
        call_repr = job_func_name + '(' + ', '.join(args + kwargs) + ')'

        if self.at_time is not None:
            return 'Every %s %s at %s do %s %s' % (
                   self.interval,
                   self.unit[:-1] if self.interval == 1 else self.unit,
                   self.at_time, call_repr, timestats)
        else:
            fmt = (
                'Every %(interval)s ' +
                ('to %(latest)s ' if self.latest is not None else '') +
                '%(unit)s do %(call_repr)s %(timestats)s'
            )

            return fmt % dict(
                interval=self.interval,
                latest=self.latest,
                unit=(self.unit[:-1] if self.interval == 1 else self.unit),
                call_repr=call_repr,
                timestats=timestats
            )

    @property
    def second(self):
        assert self.interval == 1, 'Use seconds instead of second'
        return self.seconds

    @property
    def seconds(self):
        self.unit = 'seconds'
        return self

    @property
    def minute(self):
        assert self.interval == 1, 'Use minutes instead of minute'
        return self.minutes

    @property
    def minutes(self):
        self.unit = 'minutes'
        return self

    @property
    def hour(self):
        assert self.interval == 1, 'Use hours instead of hour'
        return self.hours

    @property
    def hours(self):
        self.unit = 'hours'
        return self

    @property
    def day(self):
        assert self.interval == 1, 'Use days instead of day'
        return self.days

    @property
    def days(self):
        self.unit = 'days'
        return self

    @property
    def week(self):
        assert self.interval == 1, 'Use weeks instead of week'
        return self.weeks

    @property
    def weeks(self):
        self.unit = 'weeks'
        return self

    @property
    def monday(self):
        assert self.interval == 1, 'Use mondays instead of monday'
        self.start_day = 'monday'
        return self.weeks

    @property
    def tuesday(self):
        assert self.interval == 1, 'Use tuesdays instead of tuesday'
        self.start_day = 'tuesday'
        return self.weeks

    @property
    def wednesday(self):
        assert self.interval == 1, 'Use wedesdays instead of wednesday'
        self.start_day = 'wednesday'
        return self.weeks

    @property
    def thursday(self):
        assert self.interval == 1, 'Use thursday instead of thursday'
        self.start_day = 'thursday'
        return self.weeks

    @property
    def friday(self):
        assert self.interval == 1, 'Use fridays instead of friday'
        self.start_day = 'friday'
        return self.weeks

    @property
    def saturday(self):
        assert self.interval == 1, 'Use saturdays instead of saturday'
        self.start_day = 'saturday'
        return self.weeks

    @property
    def sunday(self):
        assert self.interval == 1, 'Use sundays instead of sunday'
        self.start_day = 'sunday'
        return self.weeks

    def tag(self, *tags):
        """
        Tags the job with one or more unique indentifiers.

        Tags must be hashable. Duplicate tags are discarded.

        :param tags: A unique list of ``Hashable`` tags.
        :return: The invoked job instance
        """
        if not all(isinstance(tag, collections.Hashable) for tag in tags):
            raise TypeError('Tags must be hashable')
        self.tags.update(tags)
        return self

    def at(self, time_str):
        """
        Schedule the job every day at a specific time.

        Calling this is only valid for jobs scheduled to run
        every N day(s).

        :param time_str: A string in `XX:YY` format.
        :return: The invoked job instance
        """
        assert self.unit in ('days', 'hours') or self.start_day
        hour, minute = time_str.split(':')
        minute = int(minute)
        if self.unit == 'days' or self.start_day:
            hour = int(hour)
            assert 0 <= hour <= 23
        elif self.unit == 'hours':
            hour = 0
        assert 0 <= minute <= 59
        self.at_time = datetime.time(hour, minute)
        return self

    def to(self, latest):
        """
        Schedule the job to run at an irregular (randomized) interval.

        The job's interval will randomly vary from the value given
        to  `every` to `latest`. The range defined is inclusive on
        both ends. For example, `every(A).to(B).seconds` executes
        the job function every N seconds such that A <= N <= B.

        :param latest: Maximum interval between randomized job runs
        :return: The invoked job instance
        """
        self.latest = latest
        return self

    def do(self, job_func, *args, **kwargs):
        """
        Specifies the job_func that should be called every time the
        job runs.

        Any additional arguments are passed on to job_func when
        the job runs.

        :param job_func: The function to be scheduled
        :return: The invoked job instance
        """
        self.job_func = functools.partial(job_func, *args, **kwargs)
        try:
            functools.update_wrapper(self.job_func, job_func)
        except AttributeError:
            # job_funcs already wrapped by functools.partial won't have
            # __name__, __module__ or __doc__ and the update_wrapper()
            # call will fail.
            pass
        self._schedule_next_run()
        self.scheduler.jobs.append(self)
        return self

    @property
    def should_run(self):
        """
        :return: ``True`` if the job should be run now.
        """
        return datetime.datetime.now() >= self.next_run

    async def run(self):
        """
        Run the job and immediately reschedule it.

        :return: The return value returned by the `job_func`
        """
        if self.is_retry:
            self.logger.warn(f'Scheduler >> Retrying job')
        else:
            self.logger.info(f'Scheduler >> Running job')
        try:
            ret = await self.job_func()
            self.last_run = datetime.datetime.now()
            self._schedule_next_run()
            self.is_retry=False
            return ret
        except Exception:
            self.logger.error(f'Scheduler >> {format_exc()}')
            self.is_retry=True
            if(self.reschedule_on_failure):
                if(self.minutes_after_failure!=0 or self.seconds_after_failure!=0):
                    self.logger.warn("Scheduler >> Rescheduled job in %s minutes and %s seconds." % (self.minutes_after_failure, self.seconds_after_failure))
                    self.last_run = None
                    self.next_run = dt.now() + td(minutes=self.minutes_after_failure, seconds=self.seconds_after_failure)
                else:
                    self.logger.warn(f"Scheduler >> Rescheduled job to run now!")
                    self.last_run = dt.now()
                    self._schedule_next_run()

    def _schedule_next_run(self):
        """
        Compute the instant when this job should run next.
        """
        assert self.unit in ('seconds', 'minutes', 'hours', 'days', 'weeks')

        if self.latest is not None:
            assert self.latest >= self.interval
            interval = random.randint(self.interval, self.latest)
        else:
            interval = self.interval

        self.period = datetime.timedelta(**{self.unit: interval})
        self.next_run = datetime.datetime.now() + self.period
        if self.start_day is not None:
            assert self.unit == 'weeks'
            weekdays = (
                'monday',
                'tuesday',
                'wednesday',
                'thursday',
                'friday',
                'saturday',
                'sunday'
            )
            assert self.start_day in weekdays
            weekday = weekdays.index(self.start_day)
            days_ahead = weekday - self.next_run.weekday()
            if days_ahead <= 0:  # Target day already happened this week
                days_ahead += 7
            self.next_run += datetime.timedelta(days_ahead) - self.period
        if self.at_time is not None:
            assert self.unit in ('days', 'hours') or self.start_day is not None
            kwargs = {
                'minute': self.at_time.minute,
                'second': self.at_time.second,
                'microsecond': 0
            }
            if self.unit == 'days' or self.start_day is not None:
                kwargs['hour'] = self.at_time.hour
            self.next_run = self.next_run.replace(**kwargs)
            # If we are running for the first time, make sure we run
            # at the specified time *today* (or *this hour*) as well
            if not self.last_run:
                now = datetime.datetime.now()
                if (self.unit == 'days' and self.at_time > now.time() and
                        self.interval == 1):
                    self.next_run = self.next_run - datetime.timedelta(days=1)
                elif self.unit == 'hours' and self.at_time.minute > now.minute:
                    self.next_run = self.next_run - datetime.timedelta(hours=1)
        if self.start_day is not None and self.at_time is not None:
            # Let's see if we will still make that time we specified today
            if (self.next_run - datetime.datetime.now()).days >= 7:
                self.next_run -= self.period

