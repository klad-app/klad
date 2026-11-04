use egui::Pos2;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const KEEP: usize = 200;
pub const SHOWN: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Info,
    Done,
    Refused,
    Error,
}

impl Tone {
    pub fn life(self) -> Option<Duration> {
        match self {
            Tone::Info => Some(Duration::ZERO),
            Tone::Done => Some(Duration::from_millis(3500)),
            Tone::Refused => Some(Duration::from_secs(6)),
            Tone::Error => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub id: u64,
    pub tone: Tone,
    pub text: String,
    pub at: Instant,
    pub anchor: Option<Pos2>,
    pub dismissed: bool,
    pub times: u32,
}

#[derive(Default)]
pub struct Messages {
    log: VecDeque<Message>,
    next: u64,
}

impl Messages {
    pub fn say(&mut self, tone: Tone, text: impl Into<String>, anchor: Option<Pos2>, now: Instant) -> u64 {
        let text = text.into();
        if let Some(last) = self
            .log
            .back_mut()
            .filter(|m| m.tone == tone && m.text == text && !m.dismissed)
        {
            last.at = now;
            last.anchor = anchor.or(last.anchor);
            last.times += 1;
            return last.id;
        }
        self.next += 1;
        self.log.push_back(Message {
            id: self.next,
            tone,
            text,
            at: now,
            anchor,
            dismissed: false,
            times: 1,
        });
        while self.log.len() > KEEP {
            self.log.pop_front();
        }
        self.next
    }

    pub fn toasts(&self, now: Instant) -> Vec<&Message> {
        let mut shown: Vec<&Message> = self
            .log
            .iter()
            .rev()
            .filter(|m| !m.dismissed)
            .filter(|m| m.tone.life().is_none_or(|life| now.duration_since(m.at) < life))
            .take(SHOWN)
            .collect();
        shown.reverse();
        shown
    }

    pub fn latest(&self) -> Option<&Message> {
        self.log.back()
    }

    pub fn dismiss(&mut self, id: u64) {
        self.log
            .iter_mut()
            .filter(|m| m.id == id)
            .for_each(|m| m.dismissed = true);
    }

    pub fn log(&self) -> impl DoubleEndedIterator<Item = &Message> {
        self.log.iter()
    }

    pub fn next_change(&self, now: Instant) -> Option<Duration> {
        self.toasts(now)
            .iter()
            .filter_map(|m| m.tone.life().map(|life| life.saturating_sub(now.duration_since(m.at))))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn results_fade_refusals_linger_and_errors_stay_until_dismissed() {
        let t0 = Instant::now();
        let mut m = Messages::default();
        let at = Some(pos2(10.0, 20.0));
        m.say(Tone::Info, "frame 4 shown", None, t0);
        let done = m.say(Tone::Done, "copied 3 marks", at, t0);
        let refused = m.say(Tone::Refused, "nothing to duplicate on this frame", at, t0);
        let error = m.say(Tone::Error, "reading doc.json", None, t0);
        let texts = |m: &Messages, now| m.toasts(now).iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(texts(&m, t0), [done, refused, error]);
        assert_eq!(texts(&m, t0 + Duration::from_secs(4)), [refused, error]);
        assert_eq!(texts(&m, t0 + Duration::from_secs(60)), [error]);
        assert_eq!(m.latest().unwrap().id, error);
        assert_eq!(m.toasts(t0)[0].anchor, at);
        m.dismiss(error);
        assert!(texts(&m, t0 + Duration::from_secs(60)).is_empty());
        assert_eq!(m.next_change(t0), Some(Duration::from_millis(3500)));
        assert_eq!(m.log().count(), 4);
    }

    #[test]
    fn a_message_said_again_is_one_message() {
        let t0 = Instant::now();
        let mut m = Messages::default();
        let a = m.say(Tone::Error, "no watcher", None, t0);
        let b = m.say(Tone::Error, "no watcher", None, t0 + Duration::from_secs(1));
        assert_eq!(a, b);
        assert_eq!(m.latest().unwrap().times, 2);
        (0..KEEP + 20).for_each(|i| {
            m.say(Tone::Done, format!("step {i}"), None, t0);
        });
        assert_eq!(m.log().count(), KEEP);
        assert_eq!(m.toasts(t0).len(), SHOWN);
    }
}
