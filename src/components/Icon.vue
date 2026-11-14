<script setup>
defineProps({ name: { type: String, required: true } })
</script>

<script>
// 24×24 線條圖示（與 zipeditor 同一套畫法）。
const PATHS = {
  upload: 'M12 16V4M7 9l5-5 5 5M4 16v3a1 1 0 001 1h14a1 1 0 001-1v-3',
  download: 'M12 4v12M7 11l5 5 5-5M4 20h16',
  send: 'M5 12h13M13 6l6 6-6 6',
  x: 'M6 6l12 12M18 6L6 18',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 002 2h6a2 2 0 002-2l1-12M9 7V4h6v3',
  play: 'M7 5l12 7-12 7z',
  stop: 'M6 6h12v12H6z',
  plus: 'M12 5v14M5 12h14',
  music: 'M9 18V5l11-2v13M9 18a3 3 0 11-6 0 3 3 0 016 0zM20 16a3 3 0 11-6 0 3 3 0 016 0z',
  gauge: 'M4 15a8 8 0 1116 0M12 15l4-5M7 15h.01M17 15h.01',
}
</script>

<template>
  <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"
       stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path :d="PATHS[name]" />
  </svg>
</template>
