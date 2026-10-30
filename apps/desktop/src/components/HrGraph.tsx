import { useId, useMemo } from 'react';
import { areaPath, downsample, linePath, ticks, yRange, type Point } from '../lib/graph.ts';
import type { Zone } from '../lib/widget.ts';
import { sortZones } from '../lib/widget.ts';

const W = 1000;

/**
 * A heart rate line over time. The SVG stretches to the box (non-scaling
 * strokes); the axis labels are HTML, so text never stretches.
 */
export function HrGraph({
  points,
  xMin,
  xMax,
  height = 220,
  zones,
  label,
  timeLabels,
}: {
  points: Point[];
  xMin: number;
  xMax: number;
  height?: number;
  /** Faint bands behind the line. */
  zones?: Zone[];
  label: string;
  /** Labels under the x axis, left to right. */
  timeLabels?: string[];
}) {
  const gradient = useId();
  const { line, area, yMin, yMax, yTicks } = useMemo(() => {
    const visible = downsample(points, 800);
    const { yMin, yMax } = yRange(visible);
    const frame = { width: W, height, xMin, xMax, yMin, yMax };
    return {
      line: linePath(visible, frame),
      area: areaPath(visible, frame),
      yMin,
      yMax,
      yTicks: ticks(yMin, yMax),
    };
  }, [points, xMin, xMax, height]);
  const y = (v: number) => ((yMax - v) / (yMax - yMin)) * 100;
  const bands = zones ? sortZones(zones) : [];

  return (
    <figure className="hr-graph" style={{ height }} aria-label={label} role="img">
      <svg viewBox={`0 0 ${W} ${height}`} preserveAspectRatio="none" aria-hidden="true">
        <defs>
          <linearGradient id={gradient} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--uwu-pink)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--uwu-pink)" stopOpacity="0" />
          </linearGradient>
        </defs>
        {bands.map((z, i) => {
          const top = Math.min(yMax, bands[i + 1]?.min ?? yMax);
          const bottom = Math.max(yMin, z.min);
          if (top <= bottom) return null;
          const y1 = ((yMax - top) / (yMax - yMin)) * height;
          const y2 = ((yMax - bottom) / (yMax - yMin)) * height;
          return (
            <rect
              key={z.min}
              x="0"
              width={W}
              y={y1}
              height={y2 - y1}
              fill={z.color}
              opacity="0.08"
            />
          );
        })}
        {yTicks.map((v) => (
          <line
            key={v}
            className="hr-graph-grid"
            x1="0"
            x2={W}
            y1={(y(v) / 100) * height}
            y2={(y(v) / 100) * height}
            vectorEffect="non-scaling-stroke"
          />
        ))}
        <path d={area} fill={`url(#${gradient})`} />
        <path d={line} className="hr-graph-line" vectorEffect="non-scaling-stroke" />
      </svg>
      {yTicks.map((v) => (
        <span key={v} className="hr-graph-tick" style={{ top: `${y(v)}%` }}>
          {v}
        </span>
      ))}
      {timeLabels && (
        <div className="hr-graph-times" aria-hidden="true">
          {timeLabels.map((text, i) => (
            <span key={i}>{text}</span>
          ))}
        </div>
      )}
    </figure>
  );
}
