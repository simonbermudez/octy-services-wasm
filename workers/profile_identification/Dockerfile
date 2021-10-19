# Production
FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /profile-identification
WORKDIR /profile-identification
COPY "./workers/profile-identification" .
RUN pip install -r ./requirements.txt
CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]

# Development > local
#When CD to this directory
# FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
# LABEL maintainer="ben@octy.ai"
# EXPOSE 8080
# RUN mkdir -p /profile-identification
# WORKDIR /profile-identification
# COPY . /profile-identification
# RUN pip install -r /profile-identification/requirements.txt
# CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]