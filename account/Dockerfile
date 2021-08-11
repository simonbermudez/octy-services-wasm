FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /account
WORKDIR /account
COPY "./account" .
RUN pip install -r ./requirements.txt
CMD ["uvicorn", "api:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]
