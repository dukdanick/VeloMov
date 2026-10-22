//! Editing operations (split, trims, ripple, move, freeze, speed...).
//! All operations keep the timeline invariants: the main track is magnetic
//! (packed without gaps, clips overlap only under transitions), other tracks
//! never contain overlapping clips, empty secondary tracks disappear.

use super::*;
use crate::util::{new_id, Us, SEC};

pub const MIN_CLIP: Us = 33_334; // ~1 frame at 30fps

/// Where a new piece of media should go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Insert into the main track at the boundary nearest to the time.
    Main(Us),
    /// Put on a free secondary track of the right kind at this time.
    Overlay(Us),
    /// Put on a specific track (index) at this time; falls back to a new track on collision.
    Track(usize, Us),
    /// Insert above all tracks as a new track.
    NewTrackAbove(Us),
}

impl Project {
    /// Display-order position where new tracks of `kind` are inserted.
    fn new_track_pos(&self, kind: TrackKind) -> usize {
        match kind {
            TrackKind::Audio => self.tracks.len(),
            _ => 0,
        }
    }

    pub fn add_track(&mut self, kind: TrackKind) -> usize {
        let pos = self.new_track_pos(kind);
        self.tracks.insert(pos, Track::new(kind));
        pos
    }

    fn track_free(&self, ti: usize, start: Us, end: Us, ignore: u64) -> bool {
        self.tracks[ti].clips.iter().all(|c| c.id == ignore || c.end() <= start || c.start >= end)
    }

    /// First free non-main track of `kind` for [start, end); creates one if needed.
    pub fn free_track(&mut self, kind: TrackKind, start: Us, end: Us, ignore: u64) -> usize {
        // Prefer the track closest to the main track (bottom-most for visual kinds).
        let mut candidates: Vec<usize> =
            (0..self.tracks.len()).filter(|&i| self.tracks[i].kind == kind && !self.tracks[i].main && !self.tracks[i].locked).collect();
        if kind != TrackKind::Audio {
            candidates.reverse();
        }
        for i in candidates {
            if self.track_free(i, start, end, ignore) {
                return i;
            }
        }
        self.add_track(kind)
    }

    pub fn track_kind_for(&self, clip: &Clip) -> TrackKind {
        match &clip.kind {
            ClipKind::Media { media } => match self.media(*media).map(|m| m.kind) {
                Some(MediaKind::Audio) => TrackKind::Audio,
                _ => TrackKind::Video,
            },
            ClipKind::Text(_) => TrackKind::Text,
            ClipKind::Sticker(_) => TrackKind::Sticker,
            ClipKind::Effect(_) => TrackKind::Effect,
            ClipKind::Filter(_) => TrackKind::Filter,
            ClipKind::Adjust => TrackKind::Adjust,
        }
    }

    /// Re-pack the main track (magnet) and make transitions consistent.
    pub fn normalize(&mut self) {
        self.ensure_main_track();
        for t in self.tracks.iter_mut() {
            t.sort();
        }
        let mi = self.main_track_index();
        let tr = &mut self.tracks[mi];
        let n = tr.clips.len();
        if self.magnet {
            // packed back to back: transitions are centred on the cut and never change the length
            let mut t = 0;
            for c in tr.clips.iter_mut() {
                c.start = t;
                t += c.duration;
            }
        } else {
            for c in tr.clips.iter_mut() {
                c.start = c.start.max(0);
            }
        }
        for i in 0..n {
            if i + 1 >= n {
                tr.clips[i].transition = None;
                continue;
            }
            let touching = (tr.clips[i + 1].start - tr.clips[i].end()).abs() <= 2_000;
            let maxd = tr.clips[i].duration.min(tr.clips[i + 1].duration).max(1);
            match tr.clips[i].transition.as_mut() {
                Some(trn) if touching => trn.dur = trn.dur.clamp(100_000.min(maxd), maxd),
                Some(_) => tr.clips[i].transition = None,
                None => {}
            }
        }
        // Secondary tracks: no transitions, no negative times.
        for (i, t) in self.tracks.iter_mut().enumerate() {
            if i != mi {
                for c in t.clips.iter_mut() {
                    c.transition = None;
                    c.start = c.start.max(0);
                }
            }
        }
        // Drop empty secondary tracks.
        self.tracks.retain(|t| t.main || !t.clips.is_empty());
    }

    /// Insert media as a new clip.
    pub fn insert_media(&mut self, media_id: u64, target: Target, image_dur: Us) -> Option<u64> {
        let m = self.media(media_id)?.clone();
        let dur = match m.kind {
            MediaKind::Image => image_dur,
            _ => m.duration.max(MIN_CLIP),
        };
        let mut clip = Clip::new(ClipKind::Media { media: media_id }, 0, dur);
        // "append" requests (huge times) mean: after everything that is already there
        let end = self.duration();
        let target = match target {
            Target::Overlay(t) if t > i64::MAX / 4 => Target::Overlay(end),
            Target::Track(i, t) if t > i64::MAX / 4 => Target::Track(i, end),
            Target::NewTrackAbove(t) if t > i64::MAX / 4 => Target::NewTrackAbove(end),
            Target::Main(t) if t > i64::MAX / 4 && m.kind == MediaKind::Audio => Target::Main(0),
            other => other,
        };
        if m.kind == MediaKind::Audio {
            let t = match target {
                Target::Main(t) | Target::Overlay(t) | Target::Track(_, t) | Target::NewTrackAbove(t) => t,
            };
            clip.start = t.max(0);
            let ti = match target {
                Target::Track(i, _)
                    if i < self.tracks.len()
                        && self.tracks[i].kind == TrackKind::Audio
                        && self.track_free(i, clip.start, clip.end(), 0) =>
                {
                    i
                }
                _ => self.free_track(TrackKind::Audio, clip.start, clip.end(), 0),
            };
            let id = clip.id;
            self.tracks[ti].clips.push(clip);
            self.normalize();
            return Some(id);
        }
        let first_main = self.tracks[self.main_track_index()].clips.is_empty();
        let id = clip.id;
        match target {
            Target::Main(t) => {
                self.insert_into_main(clip, t);
            }
            Target::Overlay(t) => {
                clip.start = t.max(0);
                let ti = self.free_track(TrackKind::Video, clip.start, clip.end(), 0);
                self.tracks[ti].clips.push(clip);
            }
            Target::Track(i, t) => {
                clip.start = t.max(0);
                if i < self.tracks.len() && self.tracks[i].main {
                    self.insert_into_main(clip, t);
                } else if i < self.tracks.len() && self.tracks[i].kind == TrackKind::Video && self.track_free(i, clip.start, clip.end(), 0)
                {
                    self.tracks[i].clips.push(clip);
                } else {
                    let ti = self.free_track(TrackKind::Video, clip.start, clip.end(), 0);
                    self.tracks[ti].clips.push(clip);
                }
            }
            Target::NewTrackAbove(t) => {
                clip.start = t.max(0);
                let ti = self.add_track(TrackKind::Video);
                self.tracks[ti].clips.push(clip);
            }
        }
        if first_main && self.ratio == Ratio::Original && !self.tracks[self.main_track_index()].clips.is_empty() {
            self.apply_ratio(Ratio::Original);
        }
        self.normalize();
        Some(id)
    }

    /// Index in the main track where something dropped at `t` goes.
    pub fn main_insert_index(&self, t: Us) -> usize {
        let tr = &self.tracks[self.main_track_index()];
        for (i, c) in tr.clips.iter().enumerate() {
            if t < c.start + c.duration / 2 {
                return i;
            }
        }
        tr.clips.len()
    }

    /// Boundary time of the main-track insertion index.
    pub fn main_insert_time(&self, idx: usize) -> Us {
        let tr = &self.tracks[self.main_track_index()];
        if idx == 0 {
            0
        } else if idx <= tr.clips.len() {
            tr.clips[idx - 1].end()
        } else {
            tr.end()
        }
    }

    pub fn insert_into_main(&mut self, mut clip: Clip, t: Us) {
        let mi = self.main_track_index();
        clip.transition = None;
        if self.magnet {
            let idx = self.main_insert_index(t);
            self.tracks[mi].clips.insert(idx, clip);
            // keep the vec order (normalize sorts by start, so assign provisional starts)
            let mut acc = 0;
            for c in self.tracks[mi].clips.iter_mut() {
                c.start = acc;
                acc += c.duration;
            }
        } else {
            clip.start = t.max(0);
            let end = clip.end();
            if !self.track_free(mi, clip.start, end, 0) {
                clip.start = self.tracks[mi].end();
            }
            self.tracks[mi].clips.push(clip);
        }
    }

    pub fn add_clip(&mut self, mut clip: Clip, at: Us) -> u64 {
        clip.start = at.max(0);
        let kind = self.track_kind_for(&clip);
        let ti = self.free_track(kind, clip.start, clip.end(), 0);
        let id = clip.id;
        self.tracks[ti].clips.push(clip);
        self.normalize();
        id
    }

    /// Split one clip at timeline time `t`. Returns the id of the right part.
    pub fn split_clip(&mut self, id: u64, t: Us) -> Option<u64> {
        let (ti, ci) = self.find_clip(id)?;
        if self.tracks[ti].locked {
            return None;
        }
        let media_dur = self.media_dur(&self.tracks[ti].clips[ci]);
        let c = &mut self.tracks[ti].clips[ci];
        if t <= c.start + MIN_CLIP / 2 || t >= c.end() - MIN_CLIP / 2 {
            return None;
        }
        let left_len = t - c.start;
        let mut right = c.clone();
        right.id = new_id();
        right.start = t;
        right.duration = c.duration - left_len;
        // speed curves are flattened to the mean speed of each part
        if let Some(curve) = c.curve.take() {
            let p = left_len as f64 / c.duration as f64;
            let left_src = curve_integral(&curve, p) * c.duration as f64;
            let total_src = curve_integral(&curve, 1.0) * c.duration as f64;
            c.speed = (left_src / left_len as f64).max(0.05);
            right.curve = None;
            right.speed = ((total_src - left_src) / right.duration as f64).max(0.05);
        }
        if c.freeze.is_none() {
            let left_src = (left_len as f64 * c.speed).round() as Us;
            let right_src = (right.duration as f64 * right.speed).round() as Us;
            if c.reverse {
                // left shows the tail of the source range
                right.src_in = c.src_in;
                c.src_in += right_src;
            } else {
                right.src_in = c.src_in + left_src;
            }
            if media_dur > 0 {
                right.src_in = right.src_in.clamp(0, media_dur);
            }
        }
        let snapshot = c.clone();
        let bases = move |p: Prop| snapshot.base_prop(p);
        right.kf = c.kf.split(left_len, &bases);
        c.duration = left_len;
        c.transition = None;
        c.anim_out = None;
        c.fade_out = 0;
        right.anim_in = None;
        right.fade_in = 0;
        let rid = right.id;
        self.tracks[ti].clips.insert(ci + 1, right);
        self.normalize();
        Some(rid)
    }

    /// Cut the timeline ranges `cuts` out of clip `id` and close the gaps (later clips of
    /// the same track move left). Returns how much time was removed.
    pub fn remove_ranges(&mut self, id: u64, cuts: &[(Us, Us)]) -> Us {
        let Some((ti, _)) = self.find_clip(id) else { return 0 };
        if self.tracks[ti].locked {
            return 0;
        }
        let mut cuts: Vec<(Us, Us)> = cuts.iter().copied().filter(|(a, b)| b > a).collect();
        cuts.sort_by_key(|c| std::cmp::Reverse(c.0));
        let packed = self.tracks[ti].main && self.magnet;
        let mut removed = 0;
        // from the end backwards, so earlier ranges keep their times
        for (a, b) in cuts {
            let Some(c) = self.clip(id).cloned() else { break };
            let (a, b) = (a.max(c.start), b.min(c.end()));
            if b - a < MIN_CLIP / 2 {
                continue;
            }
            let near = |x: Us, y: Us| (x - y).abs() <= MIN_CLIP / 2;
            // the piece to drop: [a, b) of the clip
            let piece = if near(a, c.start) && near(b, c.end()) {
                Some(id)
            } else if near(a, c.start) {
                // silence at the start: the original id keeps the head, cut it off
                self.split_clip(id, b).map(|tail| {
                    // keep the remaining part under the original id (later ranges refer to it)
                    self.swap_ids(id, tail);
                    tail
                })
            } else if near(b, c.end()) {
                self.split_clip(id, a)
            } else {
                self.split_clip(id, a).and_then(|mid| self.split_clip(mid, b).map(|_| mid))
            };
            let Some(piece) = piece else { continue };
            let Some((pti, _)) = self.find_clip(piece) else { continue };
            let len = self.clip(piece).map(|p| p.duration).unwrap_or(0);
            let pstart = self.clip(piece).map(|p| p.start).unwrap_or(0);
            self.tracks[pti].clips.retain(|o| o.id != piece);
            if !packed {
                for o in self.tracks[pti].clips.iter_mut() {
                    if o.start >= pstart {
                        o.start -= len;
                    }
                }
            }
            removed += len;
        }
        self.normalize();
        removed
    }

    /// Exchange the ids of two clips (keeps an id pointing at the part of a clip still in use).
    fn swap_ids(&mut self, a: u64, b: u64) {
        for t in self.tracks.iter_mut() {
            for c in t.clips.iter_mut() {
                if c.id == a {
                    c.id = b;
                } else if c.id == b {
                    c.id = a;
                }
            }
        }
    }

    /// Split every unlocked clip under `t` (Ctrl+Shift+B), or only `ids`.
    pub fn split_at(&mut self, t: Us, ids: Option<&[u64]>) -> Vec<u64> {
        let targets: Vec<u64> = self
            .tracks
            .iter()
            .filter(|tr| !tr.locked)
            .flat_map(|tr| tr.clips.iter())
            .filter(|c| c.contains(t) && ids.map(|ids| ids.contains(&c.id)).unwrap_or(true))
            .map(|c| c.id)
            .collect();
        targets.into_iter().filter_map(|id| self.split_clip(id, t)).collect()
    }

    pub fn delete_clips(&mut self, ids: &[u64]) {
        // linked detached audio goes with its video when linkage is on
        let mut all: Vec<u64> = ids.to_vec();
        if self.linkage {
            for id in ids {
                if let Some(c) = self.clip(*id) {
                    if c.link != 0 {
                        let link = c.link;
                        for t in &self.tracks {
                            for o in &t.clips {
                                if o.link == link && !all.contains(&o.id) {
                                    all.push(o.id);
                                }
                            }
                        }
                    }
                }
            }
        }
        // restore audio of a video whose detached audio is deleted alone
        let mut restore = vec![];
        for id in &all {
            if let Some(c) = self.clip(*id) {
                if c.link != 0 && self.track_of(*id).map(|t| t.kind == TrackKind::Audio).unwrap_or(false) {
                    restore.push(c.link);
                }
            }
        }
        for t in self.tracks.iter_mut() {
            if t.locked {
                continue;
            }
            t.clips.retain(|c| !all.contains(&c.id));
        }
        for link in restore {
            for t in self.tracks.iter_mut() {
                for c in t.clips.iter_mut() {
                    if c.link == link {
                        c.audio_detached = false;
                        c.link = 0;
                    }
                }
            }
        }
        self.normalize();
    }

    /// Q: delete the part of the clip left of `t`.
    pub fn trim_left_at(&mut self, id: u64, t: Us) -> bool {
        let Some(c) = self.clip(id) else { return false };
        if t <= c.start || t >= c.end() - MIN_CLIP / 2 {
            return false;
        }
        let new_start = t;
        self.trim_start(id, new_start, true);
        true
    }

    /// W: delete the part of the clip right of `t`.
    pub fn trim_right_at(&mut self, id: u64, t: Us) -> bool {
        let Some(c) = self.clip(id) else { return false };
        if t >= c.end() || t <= c.start + MIN_CLIP / 2 {
            return false;
        }
        self.trim_end(id, t);
        true
    }

    /// Move the in-point. With `keep_position` the clip moves too (Q behaviour on
    /// overlay tracks keeps the remaining part where it is).
    pub fn trim_start(&mut self, id: u64, new_start: Us, keep_position: bool) {
        let Some((ti, ci)) = self.find_clip(id) else { return };
        let media = self.tracks[ti].clips[ci].media_id().and_then(|m| self.media(m)).cloned();
        let is_main = self.tracks[ti].main;
        let packed = is_main && self.magnet;
        let c = &mut self.tracks[ti].clips[ci];
        let mut delta = new_start - c.start;
        delta = delta.min(c.duration - MIN_CLIP);
        let limited_source =
            matches!(media.as_ref().map(|m| m.kind), Some(MediaKind::Video) | Some(MediaKind::Audio)) && c.freeze.is_none();
        if limited_source && !c.reverse {
            // can't extend before source start
            let src_delta = (delta as f64 * c.speed).round() as Us;
            if c.src_in + src_delta < 0 {
                delta = (-(c.src_in as f64) / c.speed).round() as Us;
            }
        }
        if limited_source && c.reverse {
            let md = media.as_ref().map(|m| m.duration).unwrap_or(0);
            let tail = md - (c.src_in + c.src_len());
            let src_delta = (delta as f64 * c.speed).round() as Us;
            if -src_delta > tail {
                delta = -((tail as f64 / c.speed).round() as Us);
            }
        }
        if c.start + delta < 0 && !is_main {
            delta = -c.start;
        }
        if delta == 0 {
            return;
        }
        let snapshot = c.clone();
        let bases = move |p: Prop| snapshot.base_prop(p);
        if delta > 0 {
            c.kf.trim_start(delta, &bases);
        } else {
            for tr in c.kf.0.iter_mut() {
                for k in tr.keys.iter_mut() {
                    k.t -= delta;
                }
            }
        }
        if c.freeze.is_none() {
            let src_delta = (delta as f64 * c.speed).round() as Us;
            if !c.reverse {
                c.src_in = (c.src_in + src_delta).max(0);
            }
        }
        c.duration -= delta;
        if keep_position || !packed {
            c.start += delta;
        }
        // overlay collision check: refuse growth into a neighbour
        if !is_main {
            let (s, e, cid) = (c.start, c.end(), c.id);
            let collide = self.tracks[ti].clips.iter().any(|o| o.id != cid && o.start < e && o.end() > s);
            if collide {
                let prev_end =
                    self.tracks[ti].clips.iter().filter(|o| o.id != cid && o.end() <= e && o.start < s + 1).map(|o| o.end()).max();
                if let Some(pe) = prev_end {
                    let c = &mut self.tracks[ti].clips[ci];
                    let fix = pe - c.start;
                    if fix > 0 {
                        c.start += fix;
                        c.duration -= fix;
                        if c.freeze.is_none() && !c.reverse {
                            c.src_in += (fix as f64 * c.speed).round() as Us;
                        }
                    }
                }
            }
        }
        self.normalize();
    }

    /// Move the out-point to timeline time `new_end`.
    pub fn trim_end(&mut self, id: u64, new_end: Us) {
        let Some((ti, ci)) = self.find_clip(id) else { return };
        let media = self.tracks[ti].clips[ci].media_id().and_then(|m| self.media(m)).cloned();
        let is_main = self.tracks[ti].main;
        let next_start = if !is_main {
            let c = &self.tracks[ti].clips[ci];
            self.tracks[ti].clips.iter().filter(|o| o.id != c.id && o.start >= c.start).map(|o| o.start).min()
        } else {
            None
        };
        let c = &mut self.tracks[ti].clips[ci];
        let mut dur = (new_end - c.start).max(MIN_CLIP);
        let limited_source =
            matches!(media.as_ref().map(|m| m.kind), Some(MediaKind::Video) | Some(MediaKind::Audio)) && c.freeze.is_none();
        if limited_source {
            let md = media.as_ref().map(|m| m.duration).unwrap_or(0);
            if c.reverse {
                // growing the end consumes source before src_in
                let max_src = c.src_in + c.src_len();
                let max_dur = (max_src as f64 / c.speed).floor() as Us;
                dur = dur.min(max_dur.max(MIN_CLIP));
                let new_src_len = (dur as f64 * c.speed).round() as Us;
                let old_src_len = c.src_len();
                c.src_in = (c.src_in - (new_src_len - old_src_len)).max(0);
            } else {
                let max_dur = ((md - c.src_in) as f64 / c.speed).floor() as Us;
                dur = dur.min(max_dur.max(MIN_CLIP));
            }
        }
        if let Some(ns) = next_start {
            dur = dur.min(ns - c.start).max(MIN_CLIP);
        }
        if dur < c.duration {
            let snapshot = c.clone();
            let bases = move |p: Prop| snapshot.base_prop(p);
            c.kf.trim_end(dur, &bases);
        }
        c.duration = dur;
        self.normalize();
    }

    /// Move a clip to `new_start` on track `ti` (drag & drop). Returns the final track index.
    pub fn move_clip(&mut self, id: u64, new_start: Us, target_track: Option<usize>, main_index: Option<usize>) {
        let Some((ti, ci)) = self.find_clip(id) else { return };
        if self.tracks[ti].locked {
            return;
        }
        let mut clip = self.tracks[ti].clips.remove(ci);
        let kind = self.track_kind_for(&clip);
        let dest = target_track.unwrap_or(ti).min(self.tracks.len().saturating_sub(1));
        let dest_is_main = self.tracks.get(dest).map(|t| t.main).unwrap_or(false);
        let compatible = |t: &Track| {
            if t.main {
                kind == TrackKind::Video
            } else {
                t.kind == kind
            }
        };
        if dest_is_main && kind == TrackKind::Video && self.magnet {
            clip.transition = None;
            let mi = self.main_track_index();
            let idx = main_index.unwrap_or_else(|| self.main_insert_index(new_start)).min(self.tracks[mi].clips.len());
            self.tracks[mi].clips.insert(idx, clip);
            let mut acc = 0;
            for c in self.tracks[mi].clips.iter_mut() {
                c.start = acc;
                acc += c.duration;
            }
        } else if self.tracks.get(dest).map(|t| compatible(t)).unwrap_or(false) && !self.tracks[dest].locked {
            clip.start = new_start.max(0);
            if !dest_is_main {
                clip.transition = None;
            }
            if self.track_free(dest, clip.start, clip.end(), clip.id) {
                self.tracks[dest].clips.push(clip);
            } else {
                let nt = self.free_track(kind, clip.start, clip.end(), clip.id);
                self.tracks[nt].clips.push(clip);
            }
        } else {
            clip.start = new_start.max(0);
            clip.transition = None;
            let nt = self.free_track(kind, clip.start, clip.end(), clip.id);
            self.tracks[nt].clips.push(clip);
        }
        self.normalize();
    }

    /// Shift every clip that shares a group/link with `id` by `delta` (secondary tracks only).
    pub fn move_companions(&mut self, id: u64, delta: Us) {
        let Some(c) = self.clip(id) else { return };
        let (group, link) = (c.group, c.link);
        if delta == 0 || (group == 0 && (link == 0 || !self.linkage)) {
            return;
        }
        let mi = self.main_track_index();
        for (i, t) in self.tracks.iter_mut().enumerate() {
            if i == mi || t.locked {
                continue;
            }
            for o in t.clips.iter_mut() {
                if o.id != id && ((group != 0 && o.group == group) || (link != 0 && o.link == link)) {
                    o.start = (o.start + delta).max(0);
                }
            }
        }
        self.normalize();
    }

    /// Change constant speed keeping the source range (duration changes).
    pub fn set_speed(&mut self, id: u64, speed: f64) {
        let Some(c) = self.clip_mut(id) else { return };
        let speed = speed.clamp(0.1, 100.0);
        let src_len = c.src_len().max(1);
        let old = c.duration;
        c.curve = None;
        c.speed = speed;
        c.duration = ((src_len as f64) / speed).round().max(MIN_CLIP as f64) as Us;
        let f = c.duration as f64 / old.max(1) as f64;
        c.kf.scale_time(f);
        self.normalize();
    }

    pub fn set_curve(&mut self, id: u64, curve: Option<SpeedCurve>) {
        let Some(c) = self.clip_mut(id) else { return };
        let src_len = c.src_len().max(1);
        c.speed = 1.0;
        let old = c.duration;
        match curve {
            Some(cv) => {
                let mean = curve_mean_speed(&cv).max(0.05);
                c.duration = (src_len as f64 / mean).round() as Us;
                c.curve = Some(cv);
            }
            None => {
                c.curve = None;
                c.duration = src_len;
            }
        }
        let f = c.duration as f64 / old.max(1) as f64;
        c.kf.scale_time(f);
        self.normalize();
    }

    /// Insert a 3 s freeze frame at `t` ("Freeze").
    pub fn freeze_frame(&mut self, id: u64, t: Us) -> Option<u64> {
        let (ti, _) = self.find_clip(id)?;
        let c = self.clip(id)?.clone();
        if !matches!(c.kind, ClipKind::Media { .. }) || !c.contains(t) {
            return None;
        }
        let md = self.media_dur(&c);
        let src_t = c.source_time(t, md);
        let right = self.split_clip(id, t);
        let mut fz = c.clone();
        fz.id = new_id();
        fz.freeze = Some(src_t);
        fz.duration = 3 * SEC;
        fz.start = t;
        fz.transition = None;
        fz.anim_in = None;
        fz.anim_out = None;
        fz.anim_combo = None;
        fz.kf = Keyframes::default();
        fz.curve = None;
        fz.speed = 1.0;
        fz.fade_in = 0;
        fz.fade_out = 0;
        fz.volume = 0.0;
        let fid = fz.id;
        if self.tracks[ti].main && self.magnet {
            let mi = self.main_track_index();
            let fallback = {
                let pos = self.tracks[mi].clips.iter().position(|o| o.id == id).unwrap_or(0);
                if right.is_none() && t - c.start >= c.duration / 2 {
                    pos + 1
                } else {
                    pos
                }
            };
            let idx = right.and_then(|r| self.tracks[mi].clips.iter().position(|o| o.id == r)).unwrap_or(fallback);
            self.tracks[mi].clips.insert(idx, fz);
            let mut acc = 0;
            for c in self.tracks[mi].clips.iter_mut() {
                c.start = acc;
                acc += c.duration;
            }
        } else {
            for o in self.tracks[ti].clips.iter_mut() {
                if o.start >= t {
                    o.start += 3 * SEC;
                }
            }
            self.tracks[ti].clips.push(fz);
        }
        self.normalize();
        Some(fid)
    }

    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let (ti, ci) = self.find_clip(id)?;
        let mut c = self.tracks[ti].clips[ci].clone();
        c.id = new_id();
        c.link = 0;
        c.audio_detached = false;
        let nid = c.id;
        if self.tracks[ti].main && self.magnet {
            c.transition = None;
            self.tracks[ti].clips.insert(ci + 1, c);
            let mut acc = 0;
            for o in self.tracks[ti].clips.iter_mut() {
                o.start = acc;
                acc += o.duration;
            }
        } else {
            c.start = self.tracks[ti].clips[ci].end();
            if self.track_free(ti, c.start, c.end(), 0) {
                self.tracks[ti].clips.push(c);
            } else {
                let kind = self.track_kind_for(&c);
                let nt = self.free_track(kind, c.start, c.end(), 0);
                self.tracks[nt].clips.push(c);
            }
        }
        self.normalize();
        Some(nid)
    }

    /// Ctrl+Shift+S: separate audio into its own clip (or restore it).
    pub fn toggle_extract_audio(&mut self, id: u64) -> Option<u64> {
        let c = self.clip(id)?.clone();
        let m = self.media(c.media_id()?)?.clone();
        if m.kind != MediaKind::Video || !m.has_audio {
            return None;
        }
        if c.audio_detached && c.link != 0 {
            let link = c.link;
            let ids: Vec<u64> =
                self.tracks.iter().flat_map(|t| t.clips.iter()).filter(|o| o.link == link && o.id != id).map(|o| o.id).collect();
            for t in self.tracks.iter_mut() {
                t.clips.retain(|o| !ids.contains(&o.id));
            }
            if let Some(v) = self.clip_mut(id) {
                v.audio_detached = false;
                v.link = 0;
            }
            self.normalize();
            return None;
        }
        let link = new_id();
        let mut a = c.clone();
        a.id = new_id();
        a.link = link;
        a.transition = None;
        a.anim_in = None;
        a.anim_out = None;
        a.anim_combo = None;
        a.kf = Keyframes::default();
        if let Some(t) = c.kf.track(Prop::Volume) {
            a.kf.0.push(t.clone());
        }
        let ti = self.free_track(TrackKind::Audio, a.start, a.end(), 0);
        let aid = a.id;
        self.tracks[ti].clips.push(a);
        if let Some(v) = self.clip_mut(id) {
            v.audio_detached = true;
            v.link = link;
        }
        self.normalize();
        Some(aid)
    }

    pub fn set_transition(&mut self, clip_id: u64, tr: Option<TransitionUse>) {
        let mi = self.main_track_index();
        if let Some(c) = self.tracks[mi].clips.iter_mut().find(|c| c.id == clip_id) {
            c.transition = tr;
        }
        self.normalize();
    }

    /// Apply a transition to every cut of the main track ("Apply to all").
    pub fn transition_all(&mut self, tr: &TransitionUse) {
        let mi = self.main_track_index();
        let n = self.tracks[mi].clips.len();
        for i in 0..n.saturating_sub(1) {
            self.tracks[mi].clips[i].transition = Some(tr.clone());
        }
        self.normalize();
    }

    /// Next/previous edit point (clip boundaries and markers).
    pub fn edit_point(&self, t: Us, forward: bool) -> Option<Us> {
        let mut pts: Vec<Us> = vec![0, self.duration()];
        for tr in &self.tracks {
            for c in &tr.clips {
                pts.push(c.start);
                pts.push(c.end());
            }
        }
        pts.extend(self.markers.iter().copied());
        pts.sort();
        pts.dedup();
        if forward {
            pts.into_iter().find(|&p| p > t + 1000)
        } else {
            pts.into_iter().rev().find(|&p| p < t - 1000)
        }
    }

    /// Snap candidates for dragging (clip edges, playhead, markers).
    pub fn snap_points(&self, exclude: &[u64]) -> Vec<Us> {
        let mut pts: Vec<Us> = vec![0];
        for tr in &self.tracks {
            for c in &tr.clips {
                if exclude.contains(&c.id) {
                    continue;
                }
                pts.push(c.start);
                pts.push(c.end());
                for k in c.kf.times() {
                    pts.push(c.start + k);
                }
            }
        }
        pts.extend(self.markers.iter().copied());
        pts.sort();
        pts.dedup();
        pts
    }

    /// Copy "attributes" (copy attributes / paste attributes).
    pub fn paste_attributes(&mut self, src: &Clip, ids: &[u64]) {
        for id in ids {
            if let Some(c) = self.clip_mut(*id) {
                if c.id == src.id {
                    continue;
                }
                c.tf = src.tf;
                c.blend = src.blend;
                c.adjust = src.adjust;
                c.filter = src.filter.clone();
                c.anim_in = src.anim_in.clone();
                c.anim_out = src.anim_out.clone();
                c.anim_combo = src.anim_combo.clone();
                c.mask = src.mask;
                c.chroma = src.chroma;
                c.canvas = src.canvas;
                c.motion_blur = src.motion_blur;
                c.volume = src.volume;
                c.voice = src.voice.clone();
                if let (ClipKind::Text(dst), ClipKind::Text(s)) = (&mut c.kind, &src.kind) {
                    let text = std::mem::take(&mut dst.text);
                    *dst = s.clone();
                    dst.text = text;
                }
            }
        }
        self.normalize();
    }

    pub fn group(&mut self, ids: &[u64]) {
        if ids.len() < 2 {
            return;
        }
        let g = new_id();
        for id in ids {
            if let Some(c) = self.clip_mut(*id) {
                c.group = g;
            }
        }
    }

    pub fn ungroup(&mut self, ids: &[u64]) {
        let groups: Vec<u64> = ids.iter().filter_map(|i| self.clip(*i)).map(|c| c.group).filter(|g| *g != 0).collect();
        for t in self.tracks.iter_mut() {
            for c in t.clips.iter_mut() {
                if groups.contains(&c.group) {
                    c.group = 0;
                }
            }
        }
    }

    /// Ids of all clips grouped with `id` (including itself).
    pub fn group_members(&self, id: u64) -> Vec<u64> {
        let Some(c) = self.clip(id) else { return vec![] };
        if c.group == 0 {
            return vec![id];
        }
        let g = c.group;
        self.tracks.iter().flat_map(|t| t.clips.iter()).filter(|o| o.group == g).map(|o| o.id).collect()
    }

    /// Move a track up/down in display order (Ctrl+↑/↓ on the selected clip's track).
    pub fn move_track(&mut self, ti: usize, up: bool) {
        if ti >= self.tracks.len() || self.tracks[ti].main {
            return;
        }
        let j = if up { ti.checked_sub(1) } else { Some(ti + 1) };
        if let Some(j) = j {
            if j < self.tracks.len()
                && !self.tracks[j].main
                && (self.tracks[j].kind == TrackKind::Audio) == (self.tracks[ti].kind == TrackKind::Audio)
            {
                self.tracks.swap(ti, j);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj_with_two() -> (Project, u64, u64, u64) {
        let mut p = Project::default();
        let m = MediaAsset {
            id: 1,
            path: "a.mp4".into(),
            kind: MediaKind::Video,
            duration: 10 * SEC,
            width: 1920,
            height: 1080,
            has_video: true,
            has_audio: true,
            ..Default::default()
        };
        p.media.push(m);
        let a = p.insert_media(1, Target::Main(0), 5 * SEC).unwrap();
        let b = p.insert_media(1, Target::Main(100 * SEC), 5 * SEC).unwrap();
        (p, 1, a, b)
    }

    #[test]
    fn main_track_packs() {
        let (p, _, a, b) = proj_with_two();
        assert_eq!(p.clip(a).unwrap().start, 0);
        assert_eq!(p.clip(b).unwrap().start, 10 * SEC);
        assert_eq!(p.duration(), 20 * SEC);
    }

    #[test]
    fn split_and_delete_ripples() {
        let (mut p, _, a, b) = proj_with_two();
        let r = p.split_clip(a, 4 * SEC).unwrap();
        assert_eq!(p.clip(a).unwrap().duration, 4 * SEC);
        assert_eq!(p.clip(r).unwrap().src_in, 4 * SEC);
        p.delete_clips(&[r]);
        assert_eq!(p.clip(b).unwrap().start, 4 * SEC);
    }

    #[test]
    fn old_overlapping_transitions_are_migrated() {
        // a version 1 project: B started 1 s before A's end (the overlap was the transition)
        let (mut p, _, a, b) = proj_with_two();
        let t = p.insert_media(1, Target::Overlay(15 * SEC), 5 * SEC).unwrap();
        p.clip_mut(b).unwrap().start = 9 * SEC;
        p.clip_mut(a).unwrap().transition = Some(TransitionUse { id: "mix".into(), dur: SEC, blur: 0.0 });
        p.version = 1;
        let old_len = p.duration();
        p.migrate();
        assert_eq!(p.version, crate::model::PROJECT_VERSION);
        // cut in the middle of the old overlap; the transition covers exactly the old overlap
        assert_eq!(p.clip(a).unwrap().duration, 9 * SEC + SEC / 2);
        assert_eq!(p.clip(b).unwrap().start, 9 * SEC + SEC / 2);
        assert_eq!(p.clip(b).unwrap().src_in, SEC / 2);
        assert_eq!(p.transition_regions(), vec![(0, 9 * SEC, 10 * SEC)]);
        // nothing else moved and the length is the same
        assert_eq!(p.duration(), old_len);
        assert_eq!(p.clip(t).unwrap().start, 15 * SEC);
        p.normalize();
        assert_eq!(p.clip(b).unwrap().start, 9 * SEC + SEC / 2);
    }

    #[test]
    fn remove_ranges_closes_gaps() {
        let (mut p, _, a, b) = proj_with_two();
        // two silences inside A and one at its start; A is 10 s, B follows
        let removed = p.remove_ranges(a, &[(0, SEC), (3 * SEC, 4 * SEC), (6 * SEC, 7 * SEC)]);
        assert_eq!(removed, 3 * SEC);
        assert_eq!(p.clip(a).unwrap().start, 0);
        assert_eq!(p.clip(a).unwrap().src_in, SEC);
        assert_eq!(p.clip(b).unwrap().start, 7 * SEC);
        assert_eq!(p.duration(), 17 * SEC);
        let main = &p.tracks[p.main_track_index()].clips;
        assert_eq!(main.len(), 4);
        assert_eq!(main[1].src_in, 4 * SEC);
        assert_eq!(main[2].src_in, 7 * SEC);
    }

    #[test]
    fn transitions_keep_length() {
        let (mut p, _, a, b) = proj_with_two();
        p.set_transition(a, Some(TransitionUse { id: "mix".into(), dur: SEC, blur: 0.0 }));
        // the transition is centred on the cut: nothing moves and the length is unchanged
        assert_eq!(p.clip(b).unwrap().start, 10 * SEC);
        assert_eq!(p.duration(), 20 * SEC);
        assert_eq!(p.transition_regions(), vec![(0, 10 * SEC - SEC / 2, 10 * SEC + SEC / 2)]);
        // handles: the outgoing clip continues past its out-point, the incoming one starts early
        let ca = p.clip(a).unwrap().clone();
        let cb = p.clip(b).unwrap().clone();
        assert_eq!(ca.source_time_ext(10 * SEC + SEC / 4, 10 * SEC), 10 * SEC);
        assert_eq!(cb.source_time_ext(10 * SEC - SEC / 4, 10 * SEC), 0);
        p.trim_end(b, 13 * SEC);
        p.trim_start(b, 11 * SEC, false);
        let cb = p.clip(b).unwrap().clone();
        assert_eq!(cb.src_in, SEC);
        assert_eq!(cb.source_time_ext(cb.start - SEC / 4, 10 * SEC), SEC - SEC / 4);
        // transitions never exceed the shorter clip
        p.set_transition(a, Some(TransitionUse { id: "mix".into(), dur: 30 * SEC, blur: 0.0 }));
        assert_eq!(p.clip(a).unwrap().transition.as_ref().unwrap().dur, p.clip(b).unwrap().duration);
    }

    #[test]
    fn speed_changes_duration() {
        let (mut p, _, a, b) = proj_with_two();
        p.set_speed(a, 2.0);
        assert_eq!(p.clip(a).unwrap().duration, 5 * SEC);
        assert_eq!(p.clip(b).unwrap().start, 5 * SEC);
        assert_eq!(p.clip(a).unwrap().source_time(2 * SEC + 500_000, 10 * SEC), 5 * SEC);
    }

    #[test]
    fn audio_goes_to_audio_tracks() {
        let (mut p, _, _, _) = proj_with_two();
        let m =
            MediaAsset { id: 9, path: "a.mp3".into(), kind: MediaKind::Audio, duration: 30 * SEC, has_audio: true, ..Default::default() };
        p.media.push(m);
        // "append" on the main track and plain overlay both land on an audio track at sane times
        let a = p.insert_media(9, Target::Main(i64::MAX), 0).unwrap();
        let b = p.insert_media(9, Target::Overlay(i64::MAX), 0).unwrap();
        let ca = p.clip(a).unwrap().clone();
        let cb = p.clip(b).unwrap().clone();
        assert_eq!(p.track_of(a).unwrap().kind, TrackKind::Audio);
        assert_eq!(ca.start, 0);
        assert!(cb.start >= 0 && cb.start < 100 * SEC);
        assert_eq!(p.track_of(b).unwrap().kind, TrackKind::Audio);
        // dropping on the main video track sends audio to an audio track too
        let mi = p.main_track_index();
        let c = p.insert_media(9, Target::Track(mi, 5 * SEC), 0).unwrap();
        assert_eq!(p.track_of(c).unwrap().kind, TrackKind::Audio);
        assert_eq!(p.clip(c).unwrap().start, 5 * SEC);
    }

    #[test]
    fn q_and_w() {
        let (mut p, _, a, b) = proj_with_two();
        assert!(p.trim_left_at(a, 3 * SEC));
        let ca = p.clip(a).unwrap();
        assert_eq!(ca.start, 0);
        assert_eq!(ca.duration, 7 * SEC);
        assert_eq!(ca.src_in, 3 * SEC);
        assert!(p.trim_right_at(a, 2 * SEC));
        assert_eq!(p.clip(a).unwrap().duration, 2 * SEC);
        assert_eq!(p.clip(b).unwrap().start, 2 * SEC);
    }

    #[test]
    fn freeze() {
        let (mut p, _, a, b) = proj_with_two();
        let f = p.freeze_frame(a, 2 * SEC).unwrap();
        assert_eq!(p.clip(f).unwrap().start, 2 * SEC);
        assert_eq!(p.clip(f).unwrap().freeze, Some(2 * SEC));
        assert_eq!(p.clip(b).unwrap().start, 13 * SEC);
    }
}
