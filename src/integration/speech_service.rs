use zbus::{proxy, Result};

#[proxy(
    interface = "org.speech.Service",
    default_service = "org.speech.Service",
    default_path = "/org/speech/Service"
)]
trait SpeechService {
    /// Speak text using the default voice and backend.
    fn speak(&self, text: &str) -> Result<()>;

    /// Ask the AI cortex a question about recent speech context.
    fn think(&self, query: &str) -> Result<String>;

    /// Record audio and transcribe it using STT (fixed 4-second duration).
    fn listen(&self) -> Result<String>;

    /// Record audio using VAD (Voice Activity Detection).
    fn listen_vad(&self) -> Result<String>;

    /// Add a manual voice correction pattern.
    fn add_correction(&self, heard: &str, meant: &str) -> Result<bool>;
}

/// Helper to connect to the speech service, actively respecting the `SPEECH_SERVICE_NAME` env var.
///
/// # Errors
/// Returns a [`zbus::Error`] if the proxy destination cannot be set or the
/// proxy cannot be built against the given `conn`.
pub async fn connect(conn: &zbus::Connection) -> Result<SpeechServiceProxy<'_>> {
    let service_name =
        std::env::var("SPEECH_SERVICE_NAME").unwrap_or_else(|_| "org.speech.Service".to_string());

    SpeechServiceProxy::builder(conn)
        .destination(service_name)?
        .build()
        .await
}
