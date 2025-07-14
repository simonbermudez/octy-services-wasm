# Production
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /events
# WORKDIR /events
# COPY "./events" .
# RUN pip install -r ./requirements.txt
# ARG git_token
# RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
# CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "api/log-conf.yml"]

# Development > local
#LWhen CD to this directory
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /events
# WORKDIR /events
# COPY . /events
# ARG git_token
# RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
# RUN pip install -r /events/requirements.txt
# CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "api/log-conf.yml"]



FROM python:3.11-slim as builder
ARG git_token
RUN apt-get update && apt-get install -y git
RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0

# Final stage
FROM python:3.11-slim
LABEL maintainer="munashen@octy.ai"
EXPOSE 8080

# Copy installed packages from builder stage
COPY --from=builder /usr/local/lib/python3.11/site-packages /usr/local/lib/python3.11/site-packages

RUN mkdir -p /events
WORKDIR /events
COPY . /events

# Install additional requirements
RUN pip install -r /events/requirements.txt

CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--workers", "2", "--port", "8080", "--reload", "--log-config", "api/log-conf.yml"]