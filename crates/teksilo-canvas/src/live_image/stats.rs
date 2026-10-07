// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a source and its attachments did. Read from atomics, never under a
//! lock, so reading them never waits on a producer.

/// A source's counters, across every window that shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveImageSourceStats {
    /// The last published generation.
    pub generation: u64,
    /// The highest generation a window has presented. Screenshots do not
    /// raise it.
    pub displayed_generation: u64,
    /// Commits that published.
    pub commits: u64,
    /// Transactions dropped without `commit()`, a panic included.
    pub abandoned: u64,
    /// Bytes the producer wrote into the buffer.
    pub bytes_written: u64,
    /// Window wakes issued.
    pub wakes: u64,
    /// Changes that found every attachment's flag already raised: merged
    /// into a wake already on its way.
    pub wakes_coalesced: u64,
    /// Live writer tokens of the current session.
    pub writers: u32,
    /// Attachments not yet detached.
    pub attachments: u32,
    /// Attachments whose widget was in its window's last render.
    pub observed_attachments: u32,
    /// Attachments whose window kept its texture in its last render because
    /// every widget showing the source there was paused.
    pub paused_attachments: u32,
}

/// One attachment's counters: one widget in one window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveImageAttachmentStats {
    /// The generation this widget's window holds for the source.
    pub window_generation: u64,
    /// Presented renders that drew this attachment.
    pub frames_drawn: u64,
    /// Screenshot renders that drew this attachment.
    pub captures: u64,
    /// Renders of this window that brought its texture of the source up to
    /// date: uploaded at least one rect of it, or copied the texture another
    /// window on the same GPU device already held of its latest commit.
    pub uploads: u64,
    /// Renders that drew an older texture or only the background: the
    /// producer held the lock, or the size changed after layout.
    pub deferred_frames: u64,
    /// The widget was in its window's last render.
    pub observed: bool,
    /// Every widget showing the source in this window was paused in its last
    /// render, and the window kept its texture.
    pub paused: bool,
    /// Presented renders that kept the texture because of a pause. `uploads`
    /// does not move while this does.
    pub paused_frames: u64,
    /// The widget's paints. Flat while only pixels change.
    pub paints: u64,
}

/// A source's counters and one attachment's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveImageStats {
    pub source: LiveImageSourceStats,
    pub attachment: LiveImageAttachmentStats,
}
