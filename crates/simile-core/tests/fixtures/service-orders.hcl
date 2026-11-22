service "orders" {
  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./run-orders.yaml"
  }

  modules = ["./orders.hcl"]
}
