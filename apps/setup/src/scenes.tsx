import type { ReactNode } from 'react';
import { Heart, NYU, NyuFigure, NyuScene, Paw, PulseLine, Shadow, Star, Sticker } from './nyu';

// Installer scenes on the 320 × 220 canvas of the scenes in nyu.tsx.

function Canvas({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="-10 -10 340 230"
      className="nyu-host nyu-blink w-full overflow-visible"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

/** Nyu hops and tosses little hearts into a box with a pulse line on it. */
export function WorkingScene() {
  const S = { stroke: NYU.ink, strokeWidth: 6 } as const;
  return (
    <Canvas>
      <Shadow cx={150} rx={120} />
      <Sticker edge={18}>
        <path d="M206 134 L178 116 L190 102 L226 134Z" fill={NYU.kraftLight} {...S} />
        <path d="M290 134 L318 116 L306 102 L270 134Z" fill={NYU.kraftLight} {...S} />
        <path d="M200 134 H296 L288 200 H208Z" fill={NYU.kraft} {...S} />
        <PulseLine x={214} y={170} width={68} />
      </Sticker>
      {[0, 1, 2].map((index) => (
        <g key={index} className="setup-toss" style={{ animationDelay: `${index * 0.55}s` }}>
          <Sticker edge={12}>
            <Heart x={118} y={92} size={0.8} />
          </Sticker>
        </g>
      ))}
      <g className="setup-hop">
        <NyuFigure
          mood="happy"
          x={92}
          y={134}
          scale={0.52}
          tilt={-4}
          edge={30}
          front={<Paw x={236} y={100} />}
        />
      </g>
      <Sticker edge={12}>
        <Star x={292} y={44} r={11} />
        <Star x={30} y={40} r={8} />
      </Sticker>
    </Canvas>
  );
}

export function GoodbyeScene() {
  return <NyuScene name="goodbye" className="w-full" />;
}

export function WelcomeScene() {
  return <NyuScene name="welcome" className="w-full" />;
}

export function DoneScene() {
  return (
    <div className="animate-pop w-full">
      <NyuScene name="done" className="w-full" />
    </div>
  );
}

export function ErrorScene() {
  return <NyuScene name="loadError" className="w-full" />;
}

export function PuzzledScene() {
  return <NyuScene name="puzzled" className="w-full" />;
}
