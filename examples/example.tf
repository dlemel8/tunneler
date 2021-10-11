variable "SELECTED_EXAMPLE" {
  type    = string
  default = ""
}

variable "REDIS_PASSWORD" {
  type    = string
  default = ""
}

variable "CA_PRIVATE_KEY" {
  type    = string
  default = ""
}

variable "CA_CERTIFICATE" {
  type    = string
  default = ""
}

module "speed_test" {
  count          = var.SELECTED_EXAMPLE == "speed_test" ? 1 : 0
  source         = "./speed_test"
  CA_PRIVATE_KEY = var.CA_PRIVATE_KEY
  CA_CERTIFICATE = var.CA_CERTIFICATE
}

module "authoritative_dns" {
  count          = var.SELECTED_EXAMPLE == "authoritative_dns" ? 1 : 0
  source         = "./authoritative_dns"
  PUBLIC_IP      = linode_instance.tunneler_example.ip_address
  REDIS_PASSWORD = var.REDIS_PASSWORD
}