/**
 * Nyu for the installer, from @uwusuite/design: the palette, the sticker edge,
 * the face and the paws are the package's. UwUPulse's Nyu is the suite's box
 * cat (`<Nyu shell="box">` in the app); here she is drawn as a group so the
 * scenes can place, tilt and dress her. Her box carries a pulse line instead
 * of the suite's heart, and the props are UwUPulse's: hearts, a pulse line,
 * a little watch.
 *
 * Self-contained on purpose: the desktop app has no scene components of its
 * own, so nothing here reaches into apps/desktop.
 *
 * The colours are fixed artwork, not theme tokens: a sticker looks like itself
 * on any ground, and the white die-cut edge keeps the outlines readable.
 */

import { NYU, NyuFace, NyuPaw as Paw, Sticker, type NyuMood } from '@uwusuite/design';
import type { ReactNode } from 'react';

export { NYU, Paw, Sticker };

// Every scene is drawn on a 320 × 220 canvas, like the other UwU apps'. Nyu
// sits at about 0.6 scale, so props use a 6 px outline and an 18 px edge.
const S = { stroke: NYU.ink, strokeWidth: 6 } as const;
const EDGE = 18;
/** Nyu's own edge at scene scale: 30 × 0.6 ≈ the props' 18 px. */
const NYU_EDGE = 30;

/** A heartbeat trace from (x, y), `width` wide: flat, a small bump, the spike, flat again. */
function beat(x: number, y: number, width: number) {
  const k = width / 84;
  const points: [number, number][] = [
    [14, 0],
    [19, -5],
    [24, 0],
    [30, 0],
    [36, -26],
    [44, 18],
    [50, 0],
    [58, 0],
    [63, -6],
    [69, 0],
    [84, 0],
  ];
  return `M${x} ${y} ` + points.map(([px, py]) => `L${x + px * k} ${y + py}`).join(' ');
}

type FigureProps = {
  mood?: NyuMood;
  /** Centre of the box cat in the parent's coordinates. */
  x?: number;
  y?: number;
  /** 1 is the size of the app symbol: the box is 236 wide with its flaps. */
  scale?: number;
  tilt?: number;
  /** Extra parts in Nyu's own 256 grid. */
  behind?: ReactNode;
  front?: ReactNode;
  /** The white die-cut edge, in Nyu's own coordinates. */
  edge?: number;
};

/** The box cat as a group: peeking out of her box, a pulse line on its front. */
export function NyuFigure({
  mood = 'uwu',
  x = 128,
  y = 147,
  scale = 1,
  tilt = 0,
  behind,
  front,
  edge = 20,
}: FigureProps) {
  const ear = { fill: NYU.body, stroke: NYU.ink, strokeWidth: 9 } as const;
  const paw = { fill: NYU.body, stroke: NYU.ink, strokeWidth: 8 } as const;
  return (
    <g
      transform={`translate(${x} ${y}) rotate(${tilt}) scale(${scale}) translate(-128 -147)`}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <Sticker edge={edge}>
        {behind}
        <path
          d="M50 152 L60 128 H196 L206 152 Z"
          fill={NYU.kraftDark}
          stroke={NYU.ink}
          strokeWidth={8}
        />
        <g className="nyu-ear nyu-ear-l">
          <path d="M60 92 L78 34 L116 76 Z" {...ear} />
          <path d="M74 82 L82 52 L100 76 Z" fill={NYU.flap} />
        </g>
        <g className="nyu-ear nyu-ear-r">
          <path d="M196 92 L178 34 L140 76 Z" {...ear} />
          <path d="M182 82 L174 52 L156 76 Z" fill={NYU.flap} />
        </g>
        <rect x={42} y={62} width={172} height={130} rx={52} {...ear} />
        <NyuFace mood={mood} dy={-46} />
        <path
          d="M44 152 L10 126 L24 108 L70 136 Z"
          fill={NYU.kraftLight}
          stroke={NYU.ink}
          strokeWidth={8}
        />
        <path
          d="M212 152 L246 126 L232 108 L186 136 Z"
          fill={NYU.kraftLight}
          stroke={NYU.ink}
          strokeWidth={8}
        />
        <rect
          x={36}
          y={146}
          width={184}
          height={88}
          rx={12}
          fill={NYU.kraft}
          {...S}
          strokeWidth={9}
        />
        <path
          className="no-edge setup-beat"
          d={beat(70, 196, 116)}
          fill="none"
          stroke={NYU.body}
          strokeWidth={7}
        />
        <g className="nyu-paws">
          <ellipse cx={92} cy={148} rx={17} ry={13} {...paw} />
          <path d="M88 150 v6 M96 150 v6" stroke={NYU.ink} strokeWidth={4.5} fill="none" />
          <ellipse cx={164} cy={148} rx={17} ry={13} {...paw} />
          <path d="M160 150 v6 M168 150 v6" stroke={NYU.ink} strokeWidth={4.5} fill="none" />
        </g>
        {front}
      </Sticker>
    </g>
  );
}

export function Shadow({ cx = 160, rx = 104 }: { cx?: number; rx?: number }) {
  return (
    <ellipse className="no-edge" cx={cx} cy="204" rx={rx} ry="8" fill={NYU.ink} opacity="0.08" />
  );
}

export function Star({ x, y, r = 12 }: { x: number; y: number; r?: number }) {
  const k = r * 0.2;
  return (
    <path
      d={`M${x} ${y - r} Q${x + k} ${y - k} ${x + r} ${y} Q${x + k} ${y + k} ${x} ${y + r} Q${x - k} ${y + k} ${x - r} ${y} Q${x - k} ${y - k} ${x} ${y - r}Z`}
      fill={NYU.star}
      stroke={NYU.ink}
      strokeWidth={r > 10 ? 4 : 3}
    />
  );
}

export function Heart({
  x,
  y,
  size = 1,
  fill = NYU.body,
  className,
}: {
  x: number;
  y: number;
  size?: number;
  fill?: string;
  className?: string;
}) {
  return (
    <g transform={`translate(${x} ${y}) scale(${size})`}>
      <path
        className={className}
        d="M0 13 C-15 3 -18 -4 -17 -8 C-16 -15 -7 -16 -3 -11 L0 -8 L3 -11 C7 -16 16 -15 17 -8 C18 -4 15 3 0 13Z"
        fill={fill}
        stroke={NYU.ink}
        strokeWidth={4 / size}
      />
    </g>
  );
}

/** A pulse line on its own, `width` wide, starting at x/y. */
export function PulseLine({
  x,
  y,
  width = 84,
  color = NYU.body,
}: {
  x: number;
  y: number;
  width?: number;
  color?: string;
}) {
  return (
    <path className="setup-beat" d={beat(x, y, width)} fill="none" stroke={color} strokeWidth={5} />
  );
}

/** A little smartwatch with a heart on its face, the kind that sends the pulse. */
export function Watch({ x, y, rotate = 0 }: { x: number; y: number; rotate?: number }) {
  return (
    <g transform={`translate(${x} ${y}) rotate(${rotate})`}>
      <rect x="-11" y="-38" width="22" height="18" rx="5" fill={NYU.lilac} {...S} strokeWidth={5} />
      <rect x="-11" y="20" width="22" height="18" rx="5" fill={NYU.lilac} {...S} strokeWidth={5} />
      <rect x="-21" y="-24" width="42" height="48" rx="12" fill={NYU.ink} {...S} strokeWidth={5} />
      <path d="M21 -6 h4 v10 h-4" fill={NYU.flap} stroke={NYU.ink} strokeWidth={3} />
      <Heart x={0} y={1} size={0.62} className="setup-heartbeat" />
    </g>
  );
}

/** First start: Nyu says hello, a watch and a heart next to her. */
function Welcome() {
  return (
    <>
      <Shadow />
      <path
        d="M246 44 q12 9 10 25 M262 32 q16 13 14 35"
        fill="none"
        stroke={NYU.ink}
        strokeWidth={4}
        opacity="0.4"
      />
      <NyuFigure
        mood="happy"
        x={150}
        y={128}
        scale={0.6}
        tilt={-6}
        edge={NYU_EDGE}
        front={<Paw x={238} y={96} className="nyu-wave" />}
      />
      <Sticker edge={12}>
        <Heart x={50} y={62} size={0.95} className="setup-heartbeat" />
        <Star x={286} y={150} r={11} />
        <Star x={36} y={150} r={8} />
      </Sticker>
      <Sticker edge={EDGE}>
        <Watch x={262} y={168} rotate={18} />
      </Sticker>
    </>
  );
}

const CONFETTI: [x: number, y: number, rotate: number, fill: string][] = [
  [42, 40, -20, NYU.body],
  [78, 16, 30, NYU.star],
  [118, 30, 70, NYU.mint],
  [210, 22, -40, NYU.lilac],
  [250, 44, 15, NYU.body],
  [284, 20, 60, NYU.sky],
  [30, 108, 45, NYU.sky],
  [292, 104, -30, NYU.star],
  [48, 170, 20, NYU.lilac],
  [276, 172, -60, NYU.mint],
];

/** Setup done: Nyu cheers with both paws up, hearts flying. */
function Done() {
  return (
    <>
      <Shadow />
      <Sticker edge={10}>
        {CONFETTI.map(([x, y, rotate, fill]) => (
          <rect
            key={`${x}-${y}`}
            x={x - 7}
            y={y - 4}
            width="14"
            height="8"
            rx="2"
            transform={`rotate(${rotate} ${x} ${y})`}
            fill={fill}
            stroke={NYU.ink}
            strokeWidth={3}
          />
        ))}
      </Sticker>
      <NyuFigure
        mood="cheer"
        x={160}
        y={132}
        scale={0.6}
        edge={NYU_EDGE}
        front={
          <>
            <Paw x={22} y={96} />
            <Paw x={234} y={96} />
          </>
        }
      />
      <Sticker edge={12}>
        <Heart x={160} y={30} size={0.9} className="setup-heartbeat" />
      </Sticker>
    </>
  );
}

/** Something failed: the pulse went flat and Nyu is sad about it. */
function LoadError() {
  return (
    <>
      <Shadow cx={150} />
      <NyuFigure mood="sad" x={128} y={128} scale={0.58} tilt={6} edge={NYU_EDGE} />
      <Sticker edge={EDGE}>
        <rect x="214" y="58" width="92" height="58" rx="12" fill={NYU.paper} {...S} />
        <path d="M226 88 H294" fill="none" stroke={NYU.violet} strokeWidth={5} />
      </Sticker>
      <Sticker edge={12}>
        <Heart x={266} y={156} size={0.8} fill={NYU.lilac} />
      </Sticker>
    </>
  );
}

/** Something needs a decision first: Nyu holds up a page with a question mark. */
function Puzzled() {
  return (
    <>
      <Shadow cx={150} />
      <NyuFigure mood="puzzled" x={112} y={128} scale={0.58} tilt={-8} edge={NYU_EDGE} />
      <Sticker edge={EDGE}>
        <g transform="rotate(8 226 118)">
          <path d="M190 58 H240 L262 80 V176 H190Z" fill={NYU.paper} {...S} />
          <path d="M240 58 V80 H262" fill={NYU.flap} {...S} />
          <path
            d="M212 106 q0 -15 15 -15 q15 0 15 13 q0 10 -13 14 v8"
            fill="none"
            stroke={NYU.body}
            strokeWidth={9}
          />
          <circle cx="229" cy="145" r="5.5" fill={NYU.body} />
        </g>
        <ellipse cx="190" cy="132" rx="12" ry="10" fill={NYU.body} {...S} />
      </Sticker>
    </>
  );
}

/** Saying goodbye: Nyu waves, a little heart floating off. */
function Goodbye() {
  return (
    <>
      <Shadow />
      <NyuFigure
        mood="sad"
        x={160}
        y={128}
        scale={0.6}
        tilt={4}
        edge={NYU_EDGE}
        front={<Paw x={240} y={96} className="nyu-wave" />}
      />
      <Sticker edge={12}>
        <Heart x={58} y={58} size={0.85} fill={NYU.lilac} />
        <Star x={280} y={40} r={9} />
      </Sticker>
    </>
  );
}

const SCENES = {
  welcome: Welcome,
  done: Done,
  loadError: LoadError,
  puzzled: Puzzled,
  goodbye: Goodbye,
} satisfies Record<string, () => ReactNode>;

export type SceneName = keyof typeof SCENES;

/** A small illustration of Nyu. Decorative only. */
export function NyuScene({ name, className }: { name: SceneName; className?: string }) {
  const Scene = SCENES[name];
  return (
    <svg
      viewBox="-10 -10 340 230"
      className={className ? `nyu-host nyu-blink ${className}` : 'nyu-host nyu-blink'}
      style={{ overflow: 'visible' }}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <Scene />
    </svg>
  );
}
