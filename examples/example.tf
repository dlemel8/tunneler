variable "SELECTED_EXAMPLE" {
  type = string
  default = ""
}

module "speed_test" {
  count = var.SELECTED_EXAMPLE == "speed_test" ? 1 : 0
  source = "./speed_test"
}

module "authoritative_dns" {
  count = var.SELECTED_EXAMPLE == "authoritative_dns" ? 1 : 0
  source = "./authoritative_dns"
}