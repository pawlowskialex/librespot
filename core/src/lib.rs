#[macro_use]
extern crate log;

use librespot_protocol as protocol;

// The TLS backends are mutually exclusive (also checked in librespot-oauth, which this crate may not use)
#[cfg(all(feature = "native-tls", feature = "__rustls"))]
compile_error!(
    "Feature \"native-tls\" is mutually exclusive with \"rustls-tls-native-roots\" and \"rustls-tls-webpki-roots\". Enable only one."
);

#[cfg(not(any(feature = "native-tls", feature = "__rustls")))]
compile_error!(
    "Either feature \"native-tls\" (default), \"rustls-tls-native-roots\" or \"rustls-tls-webpki-roots\" must be enabled for this crate."
);

#[macro_use]
mod component;

pub mod apresolve;
pub mod audio_key;
pub mod authentication;
pub mod cache;
pub mod cdn_url;
pub mod channel;
pub mod config;
mod connection;
pub mod date;
#[allow(dead_code)]
pub mod dealer;
pub mod deserialize_with;
#[doc(hidden)]
pub mod diffie_hellman;
pub mod error;
pub mod file_id;
pub mod http_client;
pub mod login5;
pub mod mercury;
pub mod packet;
mod proxytunnel;
pub mod session;
mod socket;
#[allow(dead_code)]
pub mod spclient;
pub mod spotify_id;
pub mod spotify_uri;
pub mod token;
#[doc(hidden)]
pub mod util;
pub mod version;

pub use config::SessionConfig;
pub use error::Error;
pub use file_id::FileId;
pub use session::Session;
pub use spotify_id::SpotifyId;
pub use spotify_uri::SpotifyUri;
