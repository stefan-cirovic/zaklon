//! Asking the AI engine for an answer: the request, the streamed reply read
//! as it comes, and how fast this computer reads a prompt, which decides how
//! much source text an answer gets.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tracing::info;

use super::finish::{cited_sentences, finish_answer, finish_text, Finish};
use super::sources::{MAX_SOURCES, SOURCE_CHARS};
use super::{ms, AnswerStatus, Assistant, TOO_LONG};

/// About how long the engine may take to read an answer's sources: before
/// its first word it reads all of them, and a 9B model on a six-core
/// processor reads only 15 to 20 tokens a second. Sources get as much text
/// as that takes on this computer (see `source_budget`): on the test
/// computer (Ryzen 5 1600, 9B model) about 2,400 characters of the 4,200
/// three sources hold at most, and the first word came after 45 seconds
/// instead of 82, with answers as good. Much less costs answers: with 30
/// seconds (1,600 characters, sentences picked by their words) 7 of 25
/// library questions of the evaluation were answered well, not 11 to 13.
const READ_TIME: Duration = Duration::from_secs(45);
/// Characters a token of source text, about (Serbian runs 2.7 to 3, English 4).
const CHARS_PER_TOKEN: f64 = 2.8;
/// Source text an answer gets at least, however slow the computer.
const MIN_SOURCE_CHARS: usize = 900;
/// A prompt's read time counts as a measure of the engine's speed from this
/// many tokens on.
const MEASURE_TOKENS: f64 = 100.0;
/// How long the engine may take to start answering (it reads the whole prompt first).
const FIRST_BYTE: Duration = Duration::from_secs(240);
/// How long it may go quiet in the middle of an answer.
const STALL: Duration = Duration::from_secs(120);

impl Assistant {
    /// Characters of source text for an answer: what the engine reads in
    /// `READ_TIME` at the speed last measured with this model, or at a guess
    /// by the model's size before that (the first plan on a new engine reads
    /// about a thousand tokens and measures it).
    pub(super) fn source_budget(&self) -> usize {
        let model = self.selected().unwrap_or_default();
        let measured = self.read_speed.lock().unwrap_or_else(|p| p.into_inner()).as_ref().filter(|(m, _)| *m == model).map(|(_, s)| *s);
        source_budget(measured.unwrap_or_else(|| guessed_read_speed(&model)))
    }

    /// Learn how fast the engine reads a prompt from its `timings`, when it
    /// read enough of one to tell. Half the new measure and half the old, so
    /// one answer read while something else kept the processor busy does
    /// not decide alone.
    pub(super) fn note_read_speed(&self, timings: &serde_json::Value) {
        let Some(speed) = prompt_speed(timings) else { return };
        let Some(model) = self.selected() else { return };
        let mut known = self.read_speed.lock().unwrap_or_else(|p| p.into_inner());
        let smoothed = match known.as_ref() {
            Some((m, old)) if *m == model => (old + speed) / 2.0,
            _ => speed,
        };
        info!(per_second = format!("{speed:.1}"), smoothed = format!("{smoothed:.1}"), "assistant: prompt read");
        *known = Some((model, smoothed));
    }

    /// Ask the model, streaming its text into the answer as it comes. The
    /// engine gets `FIRST_BYTE` to start and `STALL` between pieces, and a
    /// stopped answer keeps what was written.
    pub(super) async fn stream_answer(&self, id: &str, port: u16, slot: u32, messages: Vec<serde_json::Value>, language: &'static str, finish: Finish) -> Result<(), String> {
        let body = serde_json::json!({
            "messages": messages,
            "stream": true,
            "max_tokens": 380,
            "id_slot": self.slot(slot),
            "cache_prompt": true,
            "temperature": 0.3,
            "repeat_penalty": 1.1,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        let asked = Instant::now();
        let send = self.http.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).json(&body).send();
        tokio::pin!(send);
        let res = loop {
            match tokio::time::timeout(Duration::from_millis(500), &mut send).await {
                Ok(r) => break r.map_err(|e| format!("AI engine: {e}"))?,
                Err(_) => {
                    self.stop_requested(id)?;
                    if asked.elapsed() > FIRST_BYTE {
                        return Err("the AI engine stopped answering".into());
                    }
                }
            }
        };
        if !res.status().is_success() {
            return Err(format!("AI engine replied {}", res.status()));
        }
        let source_ns: Vec<usize> = self.answer(id).map(|a| a.sources.iter().map(|s| s.n).collect()).unwrap_or_default();
        let mut stream = res.bytes_stream();
        // Bytes, not text: a letter like "č" can be split between two chunks.
        let mut buf: Vec<u8> = Vec::new();
        let mut raw = String::new();
        let mut tokens = 0u64;
        let mut first: Option<Instant> = None;
        let mut last = Instant::now();
        // Why it was stopped before the end, if it was.
        let mut stopped: Option<String> = None;
        loop {
            if let Err(why) = self.stop_requested(id) {
                stopped = Some(why);
                break;
            }
            let next = match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
                Ok(n) => n,
                Err(_) if last.elapsed() > STALL => return Err("the AI engine stopped answering".into()),
                Err(_) => continue,
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|e| format!("AI engine: {e}"))?;
            last = Instant::now();
            buf.extend_from_slice(&chunk);
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                let Some(data) = line.trim().strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                if !v["error"].is_null() {
                    let msg = v["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| v["error"].to_string());
                    return Err(format!("AI engine: {msg}"));
                }
                // The last piece says how much of the prompt was read and how fast.
                if v["timings"].is_object() {
                    let count = |k: &str| v["timings"][k].as_u64().unwrap_or(0) as u32;
                    self.update(id, |a| {
                        a.prompt_tokens = count("prompt_n");
                        a.cached_tokens = count("cache_n");
                    });
                    self.note_read_speed(&v["timings"]);
                }
                if let Some(piece) = v["choices"][0]["delta"]["content"].as_str() {
                    if !piece.is_empty() {
                        if first.is_none() {
                            first = Some(Instant::now());
                            self.update(id, |a| a.first_token_ms = ms(asked));
                        }
                        tokens += 1;
                        raw.push_str(piece);
                        let rate = first.map(|f| tokens as f64 / f.elapsed().as_secs_f64().max(0.001)).unwrap_or(0.0);
                        // A health answer shows only the sentences that name a source.
                        let shown = if finish.safety { cited_sentences(&finish_text(&raw, language), &source_ns) } else { raw.clone() };
                        self.update(id, |a| {
                            a.text = shown;
                            a.tokens_per_second = rate;
                        });
                    }
                }
            }
        }
        self.update(id, |a| {
            let done = finish_answer(&raw, &a.sources, &finish, language);
            a.text = done.text;
            a.sources = done.sources;
            a.cited = done.cited;
            a.fixed = done.fixed;
            if finish.library {
                a.grounded = done.grounded;
            }
            if a.text.is_empty() {
                a.status = AnswerStatus::Failed;
                a.error = Some(stopped.unwrap_or_else(|| "AI engine: the answer came back empty".to_string()));
            } else {
                // Stopped or not, what was written stays; the error says why it ends early.
                a.status = AnswerStatus::Done;
                a.error = stopped.filter(|why| why == TOO_LONG);
            }
        });
        Ok(())
    }
}

/// Characters of source text for an answer when the engine reads `speed`
/// tokens a second: what it reads in `READ_TIME`, at least
/// `MIN_SOURCE_CHARS` and at most what the sources hold.
fn source_budget(speed: f64) -> usize {
    let chars = (READ_TIME.as_secs_f64() * speed.max(0.0) * CHARS_PER_TOKEN) as usize;
    chars.clamp(MIN_SOURCE_CHARS, MAX_SOURCES * SOURCE_CHARS)
}

/// How fast a model reads a prompt on an ordinary computer (tokens a
/// second), until it is measured on this one.
fn guessed_read_speed(model: &str) -> f64 {
    match model {
        "qwen35-9b" => 15.0,
        "qwen35-4b" => 30.0,
        "qwen35-2b" => 60.0,
        _ => 120.0,
    }
}

/// Tokens a second the engine read a prompt at, from the `timings` of its
/// reply, when it read at least `MEASURE_TOKENS` of it.
fn prompt_speed(timings: &serde_json::Value) -> Option<f64> {
    let (n, ms) = (timings["prompt_n"].as_f64()?, timings["prompt_ms"].as_f64()?);
    (n >= MEASURE_TOKENS && ms > 0.0).then(|| n * 1000.0 / ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_computers_give_answers_less_to_read() {
        // Measured on the test computer: the 9B model reads 18 to 20 tokens a second.
        let slow = source_budget(18.0);
        assert!((2000..=2500).contains(&slow), "{slow}");
        assert_eq!(source_budget(1.0), MIN_SOURCE_CHARS);
        assert_eq!(source_budget(500.0), MAX_SOURCES * SOURCE_CHARS, "a fast computer reads everything found");
        assert!(source_budget(guessed_read_speed("qwen35-9b")) < source_budget(guessed_read_speed("qwen35-2b")));
        let t = serde_json::json!({ "cache_n": 317, "prompt_n": 850, "prompt_ms": 50_000.0 });
        assert_eq!(prompt_speed(&t), Some(17.0));
        assert_eq!(prompt_speed(&serde_json::json!({ "prompt_n": 20, "prompt_ms": 1000.0 })), None, "too few tokens to tell");
        assert_eq!(prompt_speed(&serde_json::Value::Null), None);
    }
}
