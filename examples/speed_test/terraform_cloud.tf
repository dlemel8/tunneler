terraform {
  backend "remote" {
    organization = "dlemel8"
    workspaces {
      name = "tunneler"
    }
  }
}
