terraform {
  required_providers {
    linode = {
      source  = "linode/linode"
      version = "1.16.0"
    }
  }
}

variable "LINODE_API_TOKEN" {
  type = string
}

variable "LINODE_PUBLIC_SSH_KEY" {
  type = string
}

variable "LINODE_ROOT_PASSWORD" {
  type = string
}

provider "linode" {
  token = var.LINODE_API_TOKEN
}

resource "linode_instance" "tunneler_example" {
  image           = "linode/ubuntu20.04"
  label           = "Tunneler-Example"
  region          = "eu-west"
  type            = "g6-nanode-1"
  authorized_keys = [var.LINODE_PUBLIC_SSH_KEY]
  root_pass       = var.LINODE_ROOT_PASSWORD
}