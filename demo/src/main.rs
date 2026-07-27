fn main() {
    #[cfg(feature = "server")]
    demo::server::launch();

    #[cfg(all(feature = "web", not(feature = "server")))]
    demo::client::launch();
}
