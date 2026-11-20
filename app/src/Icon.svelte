<script lang="ts">
  let { name, size = 16 }: { name: string; size?: number } = $props();
  const paths: Record<string, string> = {
    cube: "M12 2 3 7v10l9 5 9-5V7l-9-5Zm0 2.3L18.7 8 12 11.7 5.3 8 12 4.3ZM5 9.7l6 3.3v6.7l-6-3.3V9.7Zm8 10v-6.7l6-3.3v6.7l-6 3.3Z",
    user: "M12 12a4.5 4.5 0 1 0 0-9 4.5 4.5 0 0 0 0 9Zm0 2c-4.4 0-8 2.2-8 5v1h16v-1c0-2.8-3.6-5-8-5Z",
    gear: "M19.4 13a7.6 7.6 0 0 0 0-2l2-1.6-2-3.4-2.4 1a7 7 0 0 0-1.7-1L15 3.5h-4l-.3 2.5a7 7 0 0 0-1.7 1l-2.4-1-2 3.4L6.6 11a7.6 7.6 0 0 0 0 2l-2 1.6 2 3.4 2.4-1a7 7 0 0 0 1.7 1l.3 2.5h4l.3-2.5a7 7 0 0 0 1.7-1l2.4 1 2-3.4-2-1.6ZM13 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7Z",
    play: "M7 4.5v15l13-7.5-13-7.5Z",
    folder: "M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Z",
    server: "M4 4h16v6H4V4Zm0 10h16v6H4v-6Zm3-7.5a1 1 0 1 0 0 1 1 1 0 0 0 0-1Zm0 10a1 1 0 1 0 0 1 1 1 0 0 0 0-1Z",
    check: "m9.5 16.6-4.1-4.1L4 13.9l5.5 5.5L20 8.9l-1.4-1.4-9.1 9.1Z",
    x: "M6.4 5 5 6.4 10.6 12 5 17.6 6.4 19l5.6-5.6 5.6 5.6 1.4-1.4-5.6-5.6L19 6.4 17.6 5 12 10.6 6.4 5Z",
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d={paths[name]} /></svg>
