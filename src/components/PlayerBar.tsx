import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useApp } from "../lib/store";
import { useT } from "../lib/i18n";
import { IconButton, Progress } from "./ui";

/** Seconds as m:ss, for the player bar. */
function clock(ms: number): string {
  if (!ms) return "0:00";
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * Playback bar.
 *
 * The file is served through Tauri's asset protocol rather than a blob: a FLAC
 * track is tens of megabytes, and `convertFileSrc` lets the webview stream it
 * with range requests instead of reading the whole thing into memory.
 */
export default function PlayerBar() {
  const t = useT();
  const playing = useApp((state) => state.playing);
  const setPlaying = useApp((state) => state.setPlaying);
  const audio = useRef<HTMLAudioElement | null>(null);
  const [progress, setProgress] = useState(0);
  const [duration, setDuration] = useState(0);
  // Paused lives in the store: the bar is mounted once in the shell, but the
  // user's choice must not reset when they switch screens.
  const paused = useApp((state) => state.paused);
  const setPaused = useApp((state) => state.setPaused);
  // A second click on the same row has to restart the track, and `playing` alone
  // cannot tell that click apart from a re-render.
  const playNonce = useApp((state) => state.playNonce);
  const [error, setError] = useState(false);

  useEffect(() => {
    const element = audio.current;
    if (!element || !playing) return;
    setError(false);
    setProgress(0);
    // The duration belongs to the previous track until the new one loads.
    setDuration(0);
    element.src = convertFileSrc(playing.path);
    // Starting a track is an explicit click on its row, so it plays; a pause the
    // user asked for is kept.
    if (!paused) {
      element.play().then(
        () => undefined,
        () => {
          // Autoplay and unreadable files both land here; the bar says so instead
          // of looking like a click that did nothing.
          setError(true);
          setPaused(true);
        },
      );
    }
    return () => {
      element.pause();
      element.removeAttribute("src");
    };
    // `paused` is read above but must not restart the track when it changes, so
    // it is deliberately left out: a pause is handled by the audio element.
  }, [playing, playNonce]);

  if (!playing) return null;

  const toggle = () => {
    const element = audio.current;
    if (!element) return;
    if (element.paused) {
      void element.play().then(
        () => setPaused(false),
        () => setError(true),
      );
    } else {
      element.pause();
      setPaused(true);
    }
  };

  const percent = duration > 0 ? (progress / duration) * 100 : 0;

  // Clicking the bar jumps to that moment, which is what the position line is
  // for: a player that only shows where it is makes the user wait it out.
  const seek = (event: React.MouseEvent<HTMLDivElement>) => {
    const element = audio.current;
    if (!element || !Number.isFinite(element.duration) || element.duration <= 0) return;
    const box = event.currentTarget.getBoundingClientRect();
    if (box.width === 0) return;
    const ratio = Math.min(Math.max((event.clientX - box.left) / box.width, 0), 1);
    element.currentTime = ratio * element.duration;
    setProgress(element.currentTime);
  };

  return (
    <div className="player-bar animate-in">
      <audio
        ref={audio}
        onTimeUpdate={(event) => setProgress(event.currentTarget.currentTime)}
        onLoadedMetadata={(event) => setDuration(event.currentTarget.duration || 0)}
        onEnded={() => setPaused(true)}
        onError={() => setError(true)}
      />
      <IconButton
        icon={paused ? "play_arrow" : "pause"}
        label={paused ? t("lib.play") : t("lib.pause")}
        variant="filled"
        onClick={toggle}
      />
      <div className="player-names">
        <span className="lib-title">{playing.title}</span>
        <span className="lib-meta">
          {error ? t("lib.playFailed") : [playing.artist, playing.album].filter(Boolean).join(" · ")}
        </span>
      </div>
      <div className="player-progress">
        <div
          className="player-seek"
          role="slider"
          tabIndex={0}
          aria-label={t("lib.seek")}
          aria-valuemin={0}
          aria-valuemax={Math.round(duration)}
          aria-valuenow={Math.round(progress)}
          onClick={seek}
          onKeyDown={(event) => {
            const element = audio.current;
            if (!element) return;
            if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
              event.preventDefault();
              const step = event.key === "ArrowRight" ? 5 : -5;
              element.currentTime = Math.min(
                Math.max(element.currentTime + step, 0),
                element.duration || 0,
              );
            }
          }}
        >
          <Progress value={percent} />
        </div>
        <span className="lib-duration">
          {clock(progress * 1000)} / {clock(duration * 1000)}
        </span>
      </div>
      <IconButton
        icon="close"
        label={t("lib.stopPlayback")}
        size="small"
        onClick={() => setPlaying(null)}
      />
    </div>
  );
}

