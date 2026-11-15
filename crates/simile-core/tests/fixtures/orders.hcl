resource "queue" "orders" {}

async "dispatch_order" {
  uses = ["queue.orders"]
}

behavior "createOrder" {
  response { status = 201 }
  trigger "dispatch_order" {}
}
