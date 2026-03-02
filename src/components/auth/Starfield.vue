<template>
  <div class="absolute inset-0 overflow-hidden" aria-hidden="true" style="transform: translateZ(0); contain: layout style paint;">
    <!-- Base gradient -->
    <div class="absolute inset-0 bg-gradient-to-b from-primary/4 via-transparent to-muted/30" />

    <!-- Shared SVG gradient defs -->
    <svg class="absolute" width="0" height="0" aria-hidden="true">
      <defs>
        <radialGradient id="sf-glow-primary">
          <stop offset="0%" stop-color="var(--app-primary)" stop-opacity="0.35" />
          <stop offset="50%" stop-color="var(--app-primary)" stop-opacity="0.08" />
          <stop offset="100%" stop-color="var(--app-primary)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-primary-soft">
          <stop offset="0%" stop-color="var(--app-primary)" stop-opacity="0.2" />
          <stop offset="60%" stop-color="var(--app-primary)" stop-opacity="0.04" />
          <stop offset="100%" stop-color="var(--app-primary)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-cyan">
          <stop offset="0%" stop-color="rgb(34 211 238)" stop-opacity="0.25" />
          <stop offset="50%" stop-color="rgb(34 211 238)" stop-opacity="0.06" />
          <stop offset="100%" stop-color="rgb(34 211 238)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-cyan-soft">
          <stop offset="0%" stop-color="rgb(34 211 238)" stop-opacity="0.12" />
          <stop offset="60%" stop-color="rgb(34 211 238)" stop-opacity="0.02" />
          <stop offset="100%" stop-color="rgb(34 211 238)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-rose">
          <stop offset="0%" stop-color="rgb(244 114 182)" stop-opacity="0.2" />
          <stop offset="50%" stop-color="rgb(244 114 182)" stop-opacity="0.05" />
          <stop offset="100%" stop-color="rgb(244 114 182)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-rose-soft">
          <stop offset="0%" stop-color="rgb(244 114 182)" stop-opacity="0.1" />
          <stop offset="60%" stop-color="rgb(244 114 182)" stop-opacity="0.02" />
          <stop offset="100%" stop-color="rgb(244 114 182)" stop-opacity="0" />
        </radialGradient>
        <radialGradient id="sf-glow-amber">
          <stop offset="0%" stop-color="rgb(251 191 36)" stop-opacity="0.15" />
          <stop offset="50%" stop-color="rgb(251 191 36)" stop-opacity="0.04" />
          <stop offset="100%" stop-color="rgb(251 191 36)" stop-opacity="0" />
        </radialGradient>
        <!-- Edge fades using background color -->
        <linearGradient id="sf-fade-left" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stop-color="var(--app-background)" stop-opacity="1" />
          <stop offset="100%" stop-color="var(--app-background)" stop-opacity="0" />
        </linearGradient>
        <linearGradient id="sf-fade-right" x1="1" y1="0" x2="0" y2="0">
          <stop offset="0%" stop-color="var(--app-background)" stop-opacity="1" />
          <stop offset="100%" stop-color="var(--app-background)" stop-opacity="0" />
        </linearGradient>
        <linearGradient id="sf-fade-top" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--app-background)" stop-opacity="0.8" />
          <stop offset="100%" stop-color="var(--app-background)" stop-opacity="0" />
        </linearGradient>
        <linearGradient id="sf-fade-bottom" x1="0" y1="1" x2="0" y2="0">
          <stop offset="0%" stop-color="var(--app-background)" stop-opacity="1" />
          <stop offset="40%" stop-color="var(--app-background)" stop-opacity="0.6" />
          <stop offset="100%" stop-color="var(--app-background)" stop-opacity="0" />
        </linearGradient>
      </defs>
    </svg>

    <!-- BACK LAYER — nebula clouds, slowest drift -->
    <div class="absolute inset-0 pointer-events-none">
      <svg class="absolute inset-0 h-full w-full sf-drift-slow" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice" fill="none">
        <!-- Primary indigo nebulae -->
        <circle cx="160" cy="160" r="120" fill="url(#sf-glow-primary)" />
        <circle cx="1280" cy="140" r="110" fill="url(#sf-glow-primary)" />
        <circle cx="200" cy="640" r="115" fill="url(#sf-glow-primary)" />
        <circle cx="1250" cy="680" r="100" fill="url(#sf-glow-primary)" />
        <!-- Cyan nebulae -->
        <circle cx="1100" cy="80" r="90" fill="url(#sf-glow-cyan)" />
        <circle cx="350" cy="700" r="80" fill="url(#sf-glow-cyan)" />
        <circle cx="50" cy="300" r="70" fill="url(#sf-glow-cyan-soft)" />
        <circle cx="1400" cy="500" r="65" fill="url(#sf-glow-cyan-soft)" />
        <circle cx="700" cy="30" r="60" fill="url(#sf-glow-cyan-soft)" />
        <circle cx="750" cy="850" r="55" fill="url(#sf-glow-cyan-soft)" />
        <!-- Rose nebulae -->
        <circle cx="400" cy="100" r="70" fill="url(#sf-glow-rose)" />
        <circle cx="1050" cy="700" r="65" fill="url(#sf-glow-rose)" />
        <circle cx="1380" cy="200" r="55" fill="url(#sf-glow-rose-soft)" />
        <circle cx="80" cy="550" r="50" fill="url(#sf-glow-rose-soft)" />
        <!-- Amber nebulae -->
        <circle cx="300" cy="350" r="50" fill="url(#sf-glow-amber)" />
        <circle cx="1200" cy="400" r="45" fill="url(#sf-glow-amber)" />
        <circle cx="600" cy="750" r="40" fill="url(#sf-glow-amber)" />
        <!-- Soft fills distributed -->
        <circle cx="600" cy="60" r="80" fill="url(#sf-glow-primary-soft)" />
        <circle cx="880" cy="40" r="70" fill="url(#sf-glow-primary-soft)" />
        <circle cx="540" cy="780" r="75" fill="url(#sf-glow-primary-soft)" />
        <circle cx="920" cy="800" r="65" fill="url(#sf-glow-primary-soft)" />
        <circle cx="50" cy="450" r="65" fill="url(#sf-glow-primary-soft)" />
        <circle cx="1420" cy="380" r="60" fill="url(#sf-glow-primary-soft)" />
        <!-- Faint constellation bridges -->
        <g opacity="0.05">
          <path d="M300,200 Q500,320 720,280" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M720,280 Q940,240 1140,170" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M280,600 Q540,530 760,570" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M760,570 Q980,530 1100,610" stroke="var(--app-primary)" stroke-width="0.5" fill="none" />
        </g>
      </svg>
    </div>

    <!-- MID LAYER — connection lines, medium drift -->
    <div class="absolute inset-0 pointer-events-none">
      <svg class="absolute inset-0 h-full w-full sf-drift-mid" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice" fill="none">
        <!-- Cluster connection lines -->
        <g class="sf-line-draw" opacity="0.1">
          <path d="M40,120 Q130,80 220,140" stroke="var(--app-primary)" stroke-width="1" fill="none" />
          <path d="M220,140 Q310,200 180,260" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M180,260 Q100,300 60,220" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M1200,100 Q1300,60 1400,130" stroke="var(--app-primary)" stroke-width="1" fill="none" />
          <path d="M1200,100 Q1140,170 1260,220" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M1260,220 Q1360,190 1400,130" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M520,50 Q600,90 680,60" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M680,60 Q760,30 840,70" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M80,540 Q200,480 280,570" stroke="var(--app-primary)" stroke-width="1" fill="none" />
          <path d="M280,570 Q360,640 200,660" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M200,660 Q100,640 80,540" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M1100,580 Q1200,520 1320,590" stroke="var(--app-primary)" stroke-width="1" fill="none" />
          <path d="M1320,590 Q1400,640 1280,680" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M560,680 Q640,650 720,690" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
          <path d="M720,690 Q800,720 880,680" stroke="var(--app-primary)" stroke-width="0.75" fill="none" />
        </g>
        <!-- Cyan connections -->
        <g class="sf-line-draw" opacity="0.06">
          <path d="M1100,100 Q1200,150 1300,120" stroke="rgb(34 211 238)" stroke-width="0.75" fill="none" />
          <path d="M300,650 Q400,600 500,680" stroke="rgb(34 211 238)" stroke-width="0.75" fill="none" />
        </g>
        <!-- Rose connections -->
        <g class="sf-line-draw" opacity="0.05">
          <path d="M380,80 Q440,120 520,90" stroke="rgb(244 114 182)" stroke-width="0.6" fill="none" />
          <path d="M1050,650 Q1150,620 1200,680" stroke="rgb(244 114 182)" stroke-width="0.6" fill="none" />
        </g>
        <!-- Secondary glow halos -->
        <circle cx="420" cy="650" r="50" fill="url(#sf-glow-primary-soft)" />
        <circle cx="1060" cy="590" r="55" fill="url(#sf-glow-primary-soft)" />
        <circle cx="300" cy="200" r="40" fill="url(#sf-glow-cyan-soft)" />
        <circle cx="1160" cy="180" r="45" fill="url(#sf-glow-rose-soft)" />
        <circle cx="720" cy="400" r="30" fill="url(#sf-glow-primary-soft)" />
      </svg>
    </div>

    <!-- FRONT LAYER — dense starfield, fastest drift -->
    <div class="absolute inset-0 pointer-events-none">
      <svg class="absolute inset-0 h-full w-full sf-drift-fast" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice" fill="none">
        <!-- Indigo primary stars -->
        <g fill="var(--app-primary)">
          <circle cx="220" cy="140" r="4" opacity="0.6" />
          <circle cx="1200" cy="100" r="4" opacity="0.55" />
          <circle cx="280" cy="600" r="4" opacity="0.55" />
          <circle cx="1320" cy="620" r="3.5" opacity="0.45" />
          <circle cx="600" cy="90" r="3" opacity="0.35" />
          <circle cx="840" cy="70" r="3" opacity="0.3" />
          <circle cx="720" cy="730" r="3" opacity="0.3" />
          <circle cx="40" cy="120" r="3" opacity="0.45" />
          <circle cx="180" cy="270" r="3" opacity="0.4" />
          <circle cx="1400" cy="130" r="3" opacity="0.4" />
          <circle cx="80" cy="560" r="3.5" opacity="0.5" />
          <circle cx="1100" cy="610" r="3.5" opacity="0.5" />
          <circle cx="60" cy="230" r="2.5" opacity="0.35" />
          <circle cx="300" cy="210" r="2.5" opacity="0.3" />
          <circle cx="520" cy="50" r="2.5" opacity="0.3" />
          <circle cx="680" cy="60" r="2.5" opacity="0.3" />
          <circle cx="560" cy="710" r="2.5" opacity="0.28" />
          <circle cx="200" cy="690" r="2.5" opacity="0.35" />
          <circle cx="1280" cy="710" r="2.5" opacity="0.3" />
          <circle cx="30" cy="370" r="2.5" opacity="0.3" />
          <circle cx="1390" cy="350" r="2.5" opacity="0.3" />
          <circle cx="130" cy="60" r="2" opacity="0.3" />
          <circle cx="340" cy="100" r="1.5" opacity="0.2" />
          <circle cx="100" cy="170" r="1.5" opacity="0.25" />
          <circle cx="1320" cy="60" r="2" opacity="0.3" />
          <circle cx="140" cy="500" r="2" opacity="0.25" />
          <circle cx="320" cy="520" r="1.5" opacity="0.2" />
          <circle cx="1040" cy="560" r="2" opacity="0.25" />
          <circle cx="420" cy="650" r="2" opacity="0.2" />
          <circle cx="740" cy="100" r="2" opacity="0.25" />
          <circle cx="160" cy="340" r="1.5" opacity="0.15" />
          <circle cx="320" cy="360" r="1.5" opacity="0.15" />
          <circle cx="1300" cy="300" r="1.5" opacity="0.15" />
          <circle cx="480" cy="30" r="1.5" opacity="0.15" />
          <circle cx="640" cy="40" r="1.5" opacity="0.18" />
          <circle cx="880" cy="40" r="1.5" opacity="0.15" />
          <circle cx="240" cy="740" r="1.5" opacity="0.18" />
          <circle cx="1200" cy="730" r="1.5" opacity="0.18" />
        </g>
        <!-- Cyan stars -->
        <g fill="rgb(34 211 238)">
          <circle cx="1120" cy="90" r="3" opacity="0.35" />
          <circle cx="330" cy="680" r="2.5" opacity="0.3" />
          <circle cx="50" cy="310" r="2" opacity="0.25" />
          <circle cx="1410" cy="480" r="2" opacity="0.22" />
          <circle cx="700" cy="30" r="2" opacity="0.2" />
          <circle cx="400" cy="80" r="1.5" opacity="0.2" />
          <circle cx="1040" cy="50" r="1.5" opacity="0.18" />
          <circle cx="180" cy="430" r="1.5" opacity="0.18" />
          <circle cx="620" cy="780" r="1.5" opacity="0.15" />
          <circle cx="240" cy="160" r="1.5" opacity="0.15" />
          <circle cx="80" cy="180" r="1" opacity="0.12" />
          <circle cx="310" cy="300" r="1" opacity="0.1" />
          <circle cx="470" cy="700" r="1" opacity="0.1" />
          <circle cx="350" cy="450" r="0.75" opacity="0.06" />
        </g>
        <!-- Rose stars -->
        <g fill="rgb(244 114 182)">
          <circle cx="380" cy="100" r="2.5" opacity="0.28" />
          <circle cx="1060" cy="680" r="2.5" opacity="0.25" />
          <circle cx="1380" cy="210" r="2" opacity="0.2" />
          <circle cx="80" cy="580" r="2" opacity="0.2" />
          <circle cx="900" cy="50" r="2" opacity="0.18" />
          <circle cx="270" cy="120" r="1.5" opacity="0.18" />
          <circle cx="440" cy="720" r="1.5" opacity="0.14" />
          <circle cx="160" cy="450" r="1.5" opacity="0.12" />
          <circle cx="330" cy="240" r="1" opacity="0.1" />
          <circle cx="500" cy="600" r="1" opacity="0.08" />
          <circle cx="60" cy="350" r="1" opacity="0.08" />
        </g>
        <!-- Amber stars (rare) -->
        <g fill="rgb(251 191 36)">
          <circle cx="300" cy="370" r="2" opacity="0.18" />
          <circle cx="1200" cy="420" r="2" opacity="0.15" />
          <circle cx="580" cy="780" r="1.5" opacity="0.12" />
          <circle cx="860" cy="110" r="1.5" opacity="0.12" />
          <circle cx="150" cy="300" r="1.5" opacity="0.1" />
          <circle cx="440" cy="140" r="1" opacity="0.08" />
          <circle cx="1000" cy="650" r="1" opacity="0.08" />
        </g>
        <!-- White pinpoints -->
        <g fill="white">
          <circle cx="190" cy="150" r="1.5" opacity="0.3" />
          <circle cx="1250" cy="120" r="1.5" opacity="0.28" />
          <circle cx="310" cy="580" r="1.5" opacity="0.25" />
          <circle cx="1150" cy="600" r="1.5" opacity="0.22" />
          <circle cx="650" cy="70" r="1" opacity="0.2" />
          <circle cx="100" cy="400" r="1" opacity="0.2" />
          <circle cx="1360" cy="420" r="1" opacity="0.18" />
          <circle cx="660" cy="760" r="1" opacity="0.15" />
          <circle cx="420" cy="200" r="0.75" opacity="0.15" />
          <circle cx="250" cy="480" r="0.75" opacity="0.1" />
          <circle cx="700" cy="420" r="0.5" opacity="0.06" />
        </g>
        <!-- Pulsing rings on anchor stars -->
        <circle cx="220" cy="140" r="14" stroke="var(--app-primary)" stroke-width="0.5" fill="none" opacity="0.15" class="sf-pulse" />
        <circle cx="1200" cy="100" r="12" stroke="var(--app-primary)" stroke-width="0.5" fill="none" opacity="0.12" class="sf-pulse" style="animation-delay: -2s" />
        <circle cx="280" cy="600" r="12" stroke="var(--app-primary)" stroke-width="0.5" fill="none" opacity="0.1" class="sf-pulse" style="animation-delay: -3.5s" />
        <circle cx="1320" cy="620" r="10" stroke="var(--app-primary)" stroke-width="0.5" fill="none" opacity="0.1" class="sf-pulse" style="animation-delay: -5s" />
        <circle cx="1120" cy="90" r="10" stroke="rgb(34 211 238)" stroke-width="0.5" fill="none" opacity="0.1" class="sf-pulse" style="animation-delay: -1s" />
        <circle cx="330" cy="680" r="8" stroke="rgb(34 211 238)" stroke-width="0.5" fill="none" opacity="0.08" class="sf-pulse" style="animation-delay: -4s" />
        <circle cx="380" cy="100" r="8" stroke="rgb(244 114 182)" stroke-width="0.5" fill="none" opacity="0.08" class="sf-pulse" style="animation-delay: -1.5s" />
        <circle cx="600" cy="90" r="10" stroke="var(--app-primary)" stroke-width="0.5" fill="none" opacity="0.08" class="sf-pulse" style="animation-delay: -2.5s" />
        <circle cx="300" cy="370" r="6" stroke="rgb(251 191 36)" stroke-width="0.5" fill="none" opacity="0.06" class="sf-pulse" style="animation-delay: -0.5s" />
      </svg>
    </div>

    <!-- Edge fades -->
    <svg class="absolute inset-0 h-full w-full pointer-events-none" viewBox="0 0 1440 900" preserveAspectRatio="xMidYMid slice" fill="none">
      <rect x="0" y="0" width="200" height="900" fill="url(#sf-fade-left)" />
      <rect x="1240" y="0" width="200" height="900" fill="url(#sf-fade-right)" />
      <rect x="0" y="0" width="1440" height="160" fill="url(#sf-fade-top)" />
      <rect x="0" y="640" width="1440" height="260" fill="url(#sf-fade-bottom)" />
    </svg>
  </div>
</template>
