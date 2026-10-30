import { Heart } from 'lucide-react';

/** A heart that beats at `bpm` (CSS animation; still when idle or motion is reduced). */
export function BeatHeart({
  bpm,
  live,
  size = 56,
}: {
  bpm: number | null;
  live: boolean;
  size?: number;
}) {
  const period = bpm && live ? 60 / Math.max(20, Math.min(250, bpm)) : 0;
  return (
    <span
      className="beat-heart"
      data-live={live && period ? 'true' : 'false'}
      style={{ width: size, height: size, ['--beat' as string]: `${period || 1}s` }}
      aria-hidden="true"
    >
      <Heart width={size} height={size} strokeWidth={0} fill="currentColor" />
    </span>
  );
}
