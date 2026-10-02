terraform {
  required_version = ">= 1.5"
}

variable "regions" {
  type    = list(string)
  default = ["eu-west-1", "us-east-1"]
}

variable "tags" {
  type    = map(string)
  default = { owner = "corpus", purpose = "sanitizer" }
}

locals {
  names  = [for r in var.regions : "${r}-bucket"]
  by_key = { for i, r in var.regions : r => i if r != "" }
  policy = <<-EOT
    {
      "Version": "2012-10-17",
      "Statement": []
    }
  EOT
}

resource "storage_bucket" "this" {
  for_each = local.by_key
  name     = "corpus-${each.key}"
  tags     = merge(var.tags, { index = tostring(each.value) })

  dynamic "rule" {
    for_each = each.value > 0 ? [1] : []
    content {
      days = 30 * rule.value
    }
  }

  lifecycle {
    prevent_destroy = true
  }
}

module "network" {
  source = "./modules/network"
  cidrs  = [for i in range(3) : cidrsubnet("10.0.0.0/16", 8, i)]
}

output "bucket_names" {
  value = [for b in storage_bucket.this : b.name]
}
