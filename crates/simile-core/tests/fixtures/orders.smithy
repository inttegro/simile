$version: "2.0"
namespace example.orders

structure CreateOrderInput {}
structure CreateOrderOutput {}

@http(method: "POST", uri: "/orders", code: 201)
operation createOrder {
  input: CreateOrderInput
  output: CreateOrderOutput
}

service Orders {
  version: "1.0.0"
  operations: [createOrder]
}
