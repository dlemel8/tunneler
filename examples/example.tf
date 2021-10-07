variable "SELECTED_EXAMPLE" {
  type    = string
  default = ""
}

variable "REDIS_PASSWORD" {
  type    = string
  default = ""
}

variable "CA_PRIVATE_KEY_PATH" {
  type    = string
  default = ""
}

variable "CA_CERTIFICATE_PATH" {
  type    = string
  default = ""
}

module "speed_test" {
  count               = var.SELECTED_EXAMPLE == "speed_test" ? 1 : 0
  source              = "./speed_test"
  CA_PRIVATE_KEY_PATH = var.CA_PRIVATE_KEY_PATH
  CA_CERTIFICATE_PATH = var.CA_CERTIFICATE_PATH
}

module "authoritative_dns" {
  count          = var.SELECTED_EXAMPLE == "authoritative_dns" ? 1 : 0
  source         = "./authoritative_dns"
  PUBLIC_IP      = linode_instance.tunneler_example.ip_address
  REDIS_PASSWORD = var.REDIS_PASSWORD
}