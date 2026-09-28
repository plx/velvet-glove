resource "null_resource" "broken" {
  triggers = {
    foo = "bar"
