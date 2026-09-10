use std::{fs, path::Path, sync::mpsc, thread};

use anyhow::{anyhow, Context as _, Result};
use boa_engine::{object::JsObject, Context, JsValue, Source};
use tokio::sync::oneshot;

/// Handle used by request handlers to run transforms on the JS engine thread.
#[derive(Clone)]
pub struct JsTransformer {
    jobs: mpsc::Sender<Job>,
}

struct Job {
    input: serde_json::Value,
    reply: oneshot::Sender<Result<serde_json::Value>>,
}

impl JsTransformer {
    /// Spawn the dedicated JS engine thread and load the transform script.
    pub fn start(script_path: &Path) -> Result<Self> {
        let script = fs::read_to_string(script_path)
            .with_context(|| format!("failed to read script `{}`", script_path.display()))?;

        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();

        thread::Builder::new()
            .name("js-engine".to_owned())
            .spawn(move || {
                let mut engine = match JsEngine::new(&script) {
                    Ok(engine) => engine,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };

                if ready_tx.send(Ok(())).is_err() {
                    // The main thread gave up waiting; nothing to do.
                    return;
                }

                for job in jobs_rx.iter() {
                    let _ = job.reply.send(engine.transform(job.input));
                }
            })
            .context("failed to spawn the JS engine thread")?;

        ready_rx
            .recv()
            .context("the JS engine thread exited unexpectedly")??;

        Ok(Self { jobs: jobs_tx })
    }

    /// Transform a config, transparently hopping over to the JS engine thread.
    pub async fn transform(&self, input: serde_json::Value) -> Result<serde_json::Value> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.jobs
            .send(Job {
                input,
                reply: reply_tx,
            })
            .map_err(|_| anyhow!("the JS engine thread is not running"))?;

        reply_rx
            .await
            .map_err(|_| anyhow!("the JS engine thread is not running"))?
    }
}

/// The JS engine owns its [`Context`] on a dedicated thread because boa values
/// are not `Send`, so they cannot cross an `await` point or be stored in the
/// shared server state.
struct JsEngine {
    context: Context<'static>,
    main: JsObject,
}

impl JsEngine {
    fn new(script: &str) -> Result<Self> {
        let mut context = Context::default();
        context
            .eval(Source::from_bytes(script.as_bytes()))
            .map_err(|err| anyhow!("failed to evaluate the script: {err}"))?;

        let main = context
            .eval(Source::from_bytes(b"main"))
            .map_err(|err| anyhow!("failed to resolve the `main` function: {err}"))?
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow!("the script must define a `main(data)` function"))?;
        if !main.is_callable() {
            return Err(anyhow!("`main` must be a function"));
        }

        Ok(Self { context, main })
    }

    fn transform(&mut self, input: serde_json::Value) -> Result<serde_json::Value> {
        let arg = JsValue::from_json(&input, &mut self.context)
            .map_err(|err| anyhow!("failed to convert the input for the script: {err}"))?;

        let output = self
            .main
            .call(&JsValue::undefined(), &[arg], &mut self.context)
            .map_err(|err| anyhow!("`main` failed: {err}"))?;

        output
            .to_json(&mut self.context)
            .map_err(|err| anyhow!("failed to convert the script output: {err}"))
    }
}
