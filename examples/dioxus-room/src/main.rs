#[cfg(all(feature = "web", not(feature = "server")))]
mod client;
#[cfg(feature = "server")]
mod server;

fn main() {
    #[cfg(feature = "server")]
    server::launch();

    #[cfg(all(feature = "web", not(feature = "server")))]
    client::launch();
}
