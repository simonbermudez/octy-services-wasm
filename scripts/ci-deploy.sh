#! /bin/bash
# exit script when any command ran here returns with non-zero exit code
set -e

#TODO::: Update Dockerfiles to include specific path to resocures. 
# Add circleCI user ect to K8 cluster!! Re follow guide
# SET ENV VARIBALE FOR K8 and Docker

CHANGE_CHECKER=./scripts/changed-since-last-commit.py
COMMIT_SHA1=$CIRCLE_SHA1
export COMMIT_SHA1=$COMMIT_SHA1
export CIRCLE_COMPARE_URL=$CIRCLE_COMPARE_URL
echo "$KUBERNETES_CLUSTER_CERTIFICATE" | base64 --decode > cert.crt

echo "Checking for changes on master ..."

for deploy in $(jq -rc '.deployments[]' ./deployments.json ); do
        SERVICE_NAME=$(echo "$deploy" | jq .service_name | sed -e 's/^"//' -e 's/"$//')
        SERVICE_DIR=$(echo "$deploy" | jq .service_directory | sed -e 's/^"//' -e 's/"$//')
        DOCKER_IMAGE_NAME=$(echo "$deploy" | jq .docker_image_name | sed -e 's/^"//' -e 's/"$//')
        DOCKERFILE_PATH=$(echo "$deploy" | jq .dockerfile_path | sed -e 's/^"//' -e 's/"$//')
        K8_DEPLOY_YAML=$(echo "$deploy" | jq .k8_deployment_yaml | sed -e 's/^"//' -e 's/"$//')

        if [[ `python3 ${CHANGE_CHECKER} ${SERVICE_DIR}` == "True" ]]; then
                echo "Changes found in ${SERVICE_NAME} service, Publishing updated ${SERVICE_NAME} service Docker image ..."
                # Build docker image
                docker build --no-cache -t $DOCKER_IMAGE_NAME:latest -f $DOCKERFILE_PATH .
                # tag and push image to registry
                docker tag $DOCKER_IMAGE_NAME:latest $DOCKER_IMAGE_NAME:$CIRCLE_SHA1
                docker push $DOCKER_IMAGE_NAME:latest
                docker push $DOCKER_IMAGE_NAME:$CIRCLE_SHA1

                echo "Successfully published updated ${SERVICE_NAME} service Docker image!"

                echo "Redeploying ${SERVICE_NAME} service on K8 cluster ..."

                envsubst <$K8_DEPLOY_YAML >$K8_DEPLOY_YAML.out
                mv $K8_DEPLOY_YAML.out $K8_DEPLOY_YAML
                ./kubectl \
                        --kubeconfig=/dev/null \
                        --server=$KUBERNETES_SERVER \
                        --certificate-authority=cert.crt \
                        --token=$KUBERNETES_TOKEN \
                        apply -f $K8_DEPLOY_YAML

                echo "Successfully redeployed ${SERVICE_NAME} service on K8 cluster!"
        else
                echo "no changes found in ${SERVICE_NAME} service to warrant a redeployment"
        fi
done