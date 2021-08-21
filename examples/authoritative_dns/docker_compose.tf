variable "PUBLIC_IP" {
  type = string
}

variable "REDIS_PASSWORD" {
  type = string
}

resource "local_file" "docker_compose_prod" {
  content = templatefile("${path.module}/docker-compose.prod.yml.tmpl", {
    REDIS_PASSWORD = var.REDIS_PASSWORD
    PUBLIC_IP = var.PUBLIC_IP
  })
  filename = "${path.module}/docker-compose.prod.yml"
}