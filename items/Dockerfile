# Production
FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /items
WORKDIR /items
COPY "./items" .
RUN pip install -r ./requirements.txt
ARG git_token
RUN pip install git+https://$git_token@github.com/Octy-ai/octy-rabbitmq-lib.git@1.1.0
CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]

# Development > local
#LWhen CD to this directory
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /items
# WORKDIR /items
# COPY . /items
# RUN pip install -r /items/requirements.txt
# CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]