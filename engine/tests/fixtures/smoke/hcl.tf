resource "aws_s3_bucket" "logs" {
  bucket = "demo-logs"
}

variable "region" {
  default = "eu-west-1"
}
