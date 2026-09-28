resource "null_resource" "example" {
  triggers = {
    foo = "bar"
  }
}
