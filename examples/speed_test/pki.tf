variable "CA_PRIVATE_KEY" {
  type = string
}

variable "CA_CERTIFICATE" {
  type = string
}

resource "tls_private_key" "server_key" {
  algorithm = "RSA"
}

resource "tls_cert_request" "server_certificate_request" {
  key_algorithm   = tls_private_key.server_key.algorithm
  private_key_pem = tls_private_key.server_key.private_key_pem

  subject {
    common_name = "Tunneler Speed Test Server"
  }

  dns_names = [
    "server.tunneler"
  ]
}

resource "tls_locally_signed_cert" "server_certificate" {
  cert_request_pem = tls_cert_request.server_certificate_request.cert_request_pem

  ca_key_algorithm   = "RSA"
  ca_private_key_pem = var.CA_PRIVATE_KEY
  ca_cert_pem        = var.CA_CERTIFICATE

  validity_period_hours = 8760
  allowed_uses = [
    "digital_signature",
    "server_auth",
  ]
}

resource "local_file" "ca_certificate_file" {
  content  = var.CA_CERTIFICATE
  filename = "${path.module}/ca_certificate.pem"
}

resource "local_file" "server_key_file" {
  content  = tls_private_key.server_key.private_key_pem
  filename = "${path.module}/server_key.pem"
}

resource "local_file" "server_certificate_file" {
  content  = tls_locally_signed_cert.server_certificate.cert_pem
  filename = "${path.module}/server_certificate.pem"
}