<script lang="ts">
  import { session } from '../session.svelte'

  interface Props {
    /** Shown when nothing is loaded yet. */
    idle?: string
  }

  let { idle = 'Drop a .glb or .fbx, or click to browse' }: Props = $props()
  let over = $state(false)

  async function take(files: FileList | null) {
    const file = files?.[0]
    if (file) await session.importFile(file)
  }
</script>

<label
  class="dropzone"
  data-over={over}
  ondragover={(event) => {
    event.preventDefault()
    over = true
  }}
  ondragleave={() => (over = false)}
  ondrop={(event) => {
    event.preventDefault()
    over = false
    take(event.dataTransfer?.files ?? null)
  }}
>
  <input type="file" accept=".glb,.fbx" hidden onchange={(e) => take(e.currentTarget.files)} />
  {#if session.busy}
    {session.busy}…
  {:else if session.loaded}
    <strong>{session.source.sourceFileName}</strong>
    <div class="card-sub">
      {session.result?.stats.joints ?? 0} joints ·
      {(session.result?.stats.triangles ?? 0).toLocaleString()} tri ·
      {session.sourceHeight.toFixed(2)} m as imported
    </div>
  {:else}
    {idle}
  {/if}
</label>
