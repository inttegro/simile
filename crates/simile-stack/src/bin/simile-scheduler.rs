use simile_spec::ResourceKind;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    simile_stack::process::run_component(ResourceKind::Scheduler).await
}
