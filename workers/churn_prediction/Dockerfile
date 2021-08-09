FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /churn_prediction
WORKDIR /churn_prediction
COPY . /churn_prediction
RUN pip install -r /churn_prediction/requirements.txt
CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]