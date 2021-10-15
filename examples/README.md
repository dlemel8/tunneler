# Server Deployment Examples
All examples are written using Docker compose. You can run each example locally or deploy it using Terraform and Ansible.

See additional variables and more usage examples in each example directory:
* [speed_test](speed_test/README.md)
* [authoritative_dns](authoritative_dns/README.md)
* [pipeline](pipeline/README.md)

## Run Locally
### Server
Some examples need TLS keys and certificates. Open a shell on this repo root directory and run:
```sh
mkdir -p ./pki
bash pki.sh -k <your SSH private key file path> -t ./pki ca server client
```
Then, open a shell on the example directory and run:
```sh
docker-compose up
```
### Client
Run Tunneler client with the example local port. for example, for Speed Test TLS tunnel run:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=44301 \
REMOTE_ADDRESS=127.0.0.1 \
LOG_LEVEL=debug \
TUNNELED_TYPE=tcp \
CA_CERT=../../pki/ca.crt \
CERT=../../pki/client.crt \
KEY=../../pki/client.key \
SERVER_HOSTNAME=server.tunneler \
../../target/release/client tls
```
Then, run the service client. for example, for Speed Test run:
```sh
iperf3 -c 127.0.0.1 -p 8888
```

## Deploy Server
### Server
Server machine is deployed using Terraform. I selected Linode as my provider. After Linode instance is created, its 
public IP is written to Ansible inventory (and any other example specific resource) using Terraform templates.

I deployed my server with GitHub Actions, so I use Terraform Cloud workspace (with local Execution mode) to store Terraform state. You can update
[terraform cloud file](terraform_cloud.tf) to your workspace or don't set `TF_CLI_CONFIG_FILE` variable.
Select the example you want using `TF_VAR_SELECTED_EXAMPLE`. If the example needs more variables (for example, 
Authoritative DNS need a redis password), don't forget to set them too.

Open a shell on this directory and run (Authoritative DNS example):
```sh
TF_CLI_CONFIG_FILE=$PWD/terraform.rc \
TF_VAR_LINODE_API_TOKEN=<your token> \
TF_VAR_LINODE_PUBLIC_SSH_KEY=$(cat <your SSH public key file path>) \
TF_VAR_LINODE_ROOT_PASSWORD='<instance password>' \
TF_VAR_SELECTED_EXAMPLE='authoritative_dns' \
TF_VAR_REDIS_PASSWORD='<redis password>' \ 
terraform apply
```

Server machine is configured and the example application is deployed using Ansible. You need to pass the example you 
want as an extra variable so Ansible will select the correct Docker compose files:
```sh
ansible-playbook -i ansible_inventory \
  --key-file <your SSH private key file path> \
  --extra-vars "example_name=authoritative_dns"
  ansible_playbook.yml
```
### Client
Run Tunneler client. for example, for Authoritative DNS with Cloudflare DNS resolver run:
```sh
LOCAL_PORT=8888 \
REMOTE_PORT=53 \
REMOTE_ADDRESS=1.1.1.1 \
TUNNELED_TYPE=tcp \
CLIENT_SUFFIX=.<your authoritative server domain> \
READ_TIMEOUT_IN_MILLISECONDS=100 \
IDLE_CLIENT_TIMEOUT_IN_MILLISECONDS=30000 \
../target/release/client dns
```
Then, run the service client. for example, for Authoritative DNS run:
```sh
docker run --net=host --rm -it redis:6.0.12-alpine redis-cli -p 8888 -a <redis password> info
```