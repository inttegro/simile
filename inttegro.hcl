service "inttegro" {
  listen {
    port = 4242
  }

  openapi {
    url = "https://raw.githubusercontent.com/inttegro/openapi/refs/heads/master/commerce.yml"
  }
}
