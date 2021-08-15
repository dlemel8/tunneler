resource "local_file" "ansible_inventory" {
  content = templatefile("ansible_inventory.tmpl", {
    public-ip = linode_instance.tunneler_example.ip_address
  })
  filename = "ansible_inventory"
}