FROM tiangolo/uvicorn-gunicorn-fastapi:python3.7
LABEL maintainer="ben@octy.ai"
EXPOSE 8080
RUN mkdir -p /rfm
WORKDIR /rfm
COPY . /rfm
RUN pip install -r /rfm/requirements.txt
CMD ["uvicorn", "worker:app", "--host", "0.0.0.0", "--port", "8080", "--reload"]