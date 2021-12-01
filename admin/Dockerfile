# Production
FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /admin
WORKDIR /admin
COPY "./admin" .
RUN pip install -r ./requirements.txt
CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "api/log-conf.yml"]

# Development > local
#LWhen CD to this directory
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /admin
# WORKDIR /admin
# COPY . /admin
# RUN pip install -r /admin/requirements.txt
# CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload", "--log-config", "api/log-conf.yml"]