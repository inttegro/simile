service "orders" {
  description = "An emulator for order integration tests."

  listen {
    host = "127.0.0.1"
    port = 4242
  }

  openapi {
    path = "./run-orders.yaml"
  }
}

backend "queue" "orders" {
  description = "Carries accepted orders to background processors."
}

async "dispatch_order" {
  description = "Queue an accepted order for fulfillment."
  uses        = ["queue.orders"]
}

behavior "createOrder" {
  description = "Create and accept an order."

  response { status = 201 }
}

faults {
  use "http.slow" {}
}

scenario "slow_order" {
  description = "Return a valid order response slowly."

  inject "createOrder" {
    fault = "http.slow"
  }
}
