service "payments" {
  listen {
    host = "127.0.0.1"
    port = 4545
  }

  openapi {
    path = "./setup-service.yaml"
  }
}

principal "application" {
  credential "apiKey" {}
}

principal_setup "application" {
  description = "Create an application identity for this integration."
  operation   = "createApplication"

  collect "name" {
    description = "A name that identifies the integration."
    required    = true
    to          = request.name
  }

  collect "webhook_url" {
    description = "Where webhook deliveries should be sent."
    type        = "url"
    to          = request.webhook_url
    bind        = webhook.target
  }

  issue "app_id" {
    description = "The application identifier."
    from        = response.app.id
  }

  issue "api_key" {
    description = "The secret used to authenticate API requests."
    generate    = "token"
    prefix      = "sk_simile_"
    credential  = "apiKey"
    sensitive   = true
  }
}
