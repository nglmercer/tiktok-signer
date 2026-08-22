// The shared implementation is TypeScript; build `packages/tiktok-live` before running the
// headless probes. Keeping this as a re-export prevents a second player serializer from drifting.
export * from '../../../packages/tiktok-live/dist/player.js';
