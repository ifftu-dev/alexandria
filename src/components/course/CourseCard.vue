<script setup lang="ts">
import type { Course } from '@/types'
import { sanitizeSvg } from '@/utils/sanitize'

interface Props {
  course: Course
  /** 'grid' for the standard vertical card, 'compact' for a small horizontal card */
  variant?: 'grid' | 'compact'
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'grid',
})
</script>

<template>
  <router-link :to="`/courses/${course.id}`" class="cc-grid group" v-if="variant === 'grid'">
    <!-- Thumbnail -->
    <div class="cc-thumb">
      <div v-if="course.thumbnail_svg" class="cc-thumb__img" v-html="sanitizeSvg(course.thumbnail_svg)" />
      <div v-else class="cc-thumb__placeholder">
        <svg class="h-10 w-10 text-muted-foreground/35" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
        </svg>
      </div>

      <!-- Stats pills (Mark 2 glassmorphism overlay) -->
      <div v-if="course.tags?.length" class="cc-stats">
        <span class="cc-stats__pill">
          v{{ course.version }}
        </span>
      </div>
    </div>

    <!-- Body -->
    <div class="cc-body">
      <!-- Tags -->
      <div v-if="course.tags?.length" class="cc-tags">
        <span
          v-for="tag in course.tags.slice(0, 3)"
          :key="tag"
          class="cc-tag"
        >{{ tag }}</span>
        <span v-if="course.tags.length > 3" class="cc-tag cc-tag--overflow">
          +{{ course.tags.length - 3 }}
        </span>
      </div>

      <!-- Title -->
      <h3 class="cc-title">{{ course.title }}</h3>

      <!-- Description -->
      <p v-if="course.description" class="cc-desc">{{ course.description }}</p>

      <!-- Spacer to push author down -->
      <div class="flex-1" />

      <!-- Author -->
      <div class="cc-author">
        <div class="cc-avatar">
          <span>{{ (course.author_name || 'A').charAt(0).toUpperCase() }}</span>
        </div>
        <span class="cc-author__name">
          {{ course.author_name || (course.author_address ? course.author_address.slice(0, 16) + '...' : 'Unknown') }}
        </span>
      </div>
    </div>
  </router-link>
</template>

<style scoped>
/* ============================
   CourseCard — Mark 2 "grid" variant
   Borderless card with shadow, hover lift,
   glassmorphism stats, uppercase tags
   ============================ */

.cc-grid {
  display: flex;
  flex-direction: column;
  border-radius: 0.75rem;
  background: var(--app-card);
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.04), 0 4px 12px rgb(0 0 0 / 0.03);
  overflow: hidden;
  transition: transform 0.25s cubic-bezier(0.22, 1, 0.36, 1),
              box-shadow 0.25s cubic-bezier(0.22, 1, 0.36, 1);
  cursor: pointer;
  text-decoration: none;
  color: inherit;
  height: 100%;
}

.cc-grid:hover {
  transform: translateY(-3px);
  box-shadow:
    0 8px 25px rgb(0 0 0 / 0.08),
    0 2px 10px rgb(0 0 0 / 0.04),
    0 0 0 1px color-mix(in srgb, var(--app-primary) 8%, transparent);
}

:is(.dark *) .cc-grid {
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.3), 0 4px 12px rgb(0 0 0 / 0.2);
}

:is(.dark *) .cc-grid:hover {
  box-shadow:
    0 8px 25px rgb(0 0 0 / 0.4),
    0 2px 10px rgb(0 0 0 / 0.3),
    0 0 0 1px color-mix(in srgb, var(--app-primary) 12%, transparent);
}

/* Thumbnail */
.cc-thumb {
  position: relative;
  aspect-ratio: 16 / 9;
  overflow: hidden;
  background: var(--app-muted);
}

.cc-thumb__img {
  width: 100%;
  height: 100%;
  transition: transform 0.4s ease;
}

.cc-thumb__img :deep(svg),
.cc-thumb__img :deep(img) {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.cc-grid:hover .cc-thumb__img {
  transform: scale(1.04);
}

.cc-thumb__placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg,
    color-mix(in srgb, var(--app-primary) 12%, transparent),
    color-mix(in srgb, var(--app-accent) 6%, transparent)
  );
}

/* Stats pills — glassmorphism */
.cc-stats {
  position: absolute;
  bottom: 0.5rem;
  left: 0.5rem;
  display: flex;
  gap: 0.375rem;
}

.cc-stats__pill {
  display: inline-flex;
  align-items: center;
  gap: 0.25rem;
  padding: 0.1875rem 0.4375rem;
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.01em;
  color: #fff;
  background: rgb(0 0 0 / 0.55);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border-radius: 0.375rem;
}

/* Body */
.cc-body {
  display: flex;
  flex-direction: column;
  padding: 0.875rem 1rem 1rem;
  flex: 1;
}

/* Tags */
.cc-tags {
  display: flex;
  flex-wrap: nowrap;
  overflow: clip;
  gap: 0.375rem;
  margin-bottom: 0.5rem;
}

.cc-tag {
  display: inline-block;
  font-size: 0.625rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--app-primary);
  background: color-mix(in srgb, var(--app-primary) 7%, transparent);
  padding: 0.125rem 0.375rem;
  border-radius: 0.25rem;
  white-space: nowrap;
  max-width: 10rem;
  overflow: hidden;
  text-overflow: ellipsis;
}

.cc-tag--overflow {
  color: var(--app-muted-foreground);
  background: var(--app-muted);
}

/* Title */
.cc-title {
  font-size: 0.9375rem;
  font-weight: 700;
  letter-spacing: -0.01em;
  line-height: 1.35;
  color: var(--app-foreground);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  min-height: 2.6em;
  transition: color var(--transition-fast);
}

.cc-grid:hover .cc-title {
  color: var(--app-primary);
}

/* Description */
.cc-desc {
  font-size: 0.8125rem;
  line-height: 1.5;
  color: var(--app-muted-foreground);
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  margin-top: 0.25rem;
  min-height: 2.5em;
}

/* Author */
.cc-author {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin-top: 0.75rem;
  padding-top: 0.625rem;
  border-top: 1px solid color-mix(in srgb, var(--app-border) 40%, transparent);
}

.cc-avatar {
  width: 1.375rem;
  height: 1.375rem;
  border-radius: 50%;
  background: linear-gradient(135deg, var(--app-primary), var(--app-accent));
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.cc-avatar span {
  font-size: 0.5rem;
  font-weight: 700;
  color: white;
}

.cc-author__name {
  font-size: 0.75rem;
  color: var(--app-muted-foreground);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
