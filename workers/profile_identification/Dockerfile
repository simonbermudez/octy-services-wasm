# Production
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /profile_identification
# WORKDIR /profile_identification
# COPY "./workers/profile_identification" .
# RUN pip install -r ./requirements.txt
# ARG git_token
# RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
# CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "worker/log-conf.yml"]

# Development > local
#When CD to this directory
FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /profile_identification
WORKDIR /profile_identification
COPY . /profile_identification
ARG git_token
RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
RUN pip install -r /profile_identification/requirements.txt
CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "worker/log-conf.yml"]