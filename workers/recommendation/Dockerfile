# Production
FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /recommendation
WORKDIR /recommendation
COPY "./workers/recommendation" .
RUN pip install -r ./requirements.txt
ARG git_token
RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.1.0
CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]

# Development > local
#LWhen CD to this directory
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /recommendation
# WORKDIR /recommendation
# COPY . /recommendation
# RUN pip install -r /recommendation/requirements.txt
# CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]