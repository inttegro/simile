export type ServiceName = "Orders";
export type CollectionVersion = "1.0.0";

export interface OrdersOperations {
  readonly createOrder: {
    readonly method: "POST";
    readonly path: "/orders";
    readonly responses: {
      readonly "201": {
        readonly content: {
          readonly "application/json": { readonly id: string };
        };
      };
    };
  };
}
