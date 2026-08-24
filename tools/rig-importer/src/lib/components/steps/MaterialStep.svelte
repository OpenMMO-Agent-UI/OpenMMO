<script lang="ts">
  import { session } from '../../session.svelte'
  import { readMaterials } from '../../gltf/materials'

  let before = $derived(session.loaded && session.original ? readMaterials(session.original) : [])
  // Depends on `result` for reactivity; reads the container it produced.
  let after = $derived(session.result && session.container ? readMaterials(session.container) : [])

  function touch() {
    session.scheduleRecompute()
  }
</script>

<section class="panel">
  <h3>As imported</h3>
  {#each before as material (material.index)}
    <dl class="stats">
      <dt>{material.name}</dt>
      <dd></dd>
      <dt>metallic</dt>
      <dd class:bad={material.metallicFactor > 0.5 && !material.hasMetallicRoughnessTexture}>
        {material.metallicFactor.toFixed(2)}
      </dd>
      <dt>roughness</dt>
      <dd>{material.roughnessFactor.toFixed(2)}</dd>
      <dt>emissive</dt>
      <dd class:bad={material.emissive.some((c) => c > 0.01) || material.hasEmissiveTexture}>
        {material.hasEmissiveTexture ? 'texture' : material.emissive.map((c) => c.toFixed(2)).join(' ')}
      </dd>
      <dt>specular</dt>
      <dd class:bad={material.specular !== null && material.specular > 1.001}>
        {material.specular === null ? '—' : material.specular.toFixed(2)}
      </dd>
      <dt>alpha</dt>
      <dd>{material.alphaMode}</dd>
    </dl>
  {/each}
  {#if session.materialsNeedRepair}
    <p class="hint">
      <span class="badge badge-yellow">repair</span>
      A Mixamo FBX comes in at metallic 1 with <code>KHR_materials_specular</code> boosted to 2×, which
      reads as black chrome — or, over a light albedo, a washed-out pale sheen. Meshy adds an emissive
      that makes the model self-lit. All of it is fixed below.
    </p>
  {/if}
</section>

<section class="panel">
  <h3>Fix</h3>
  <div class="field">
    <label for="mat-metal">metallicFactor <span class="num">{session.settings.material.metallicFactor.toFixed(2)}</span></label>
    <input id="mat-metal" type="range" min="0" max="1" step="0.01" bind:value={session.settings.material.metallicFactor} oninput={touch} />
  </div>
  <div class="field">
    <label for="mat-rough">roughnessFactor <span class="num">{session.settings.material.roughnessFactor.toFixed(2)}</span></label>
    <input id="mat-rough" type="range" min="0" max="1" step="0.01" bind:value={session.settings.material.roughnessFactor} oninput={touch} />
  </div>
  <label class="field-inline">
    <input type="checkbox" bind:checked={session.settings.material.clearEmissive} onchange={touch} />
    Clear emissive
  </label>

  <div class="field" style="margin-top: 10px">
    <label for="spec-mode">KHR_materials_specular</label>
    <div class="segmented" id="spec-mode">
      {#each [['remove', 'Remove'], ['set', 'Set'], ['keep', 'Keep as imported']] as [mode, label] (mode)}
        <button
          aria-pressed={session.settings.material.specularMode === mode}
          onclick={() => {
            session.settings.material.specularMode = mode as 'remove' | 'set' | 'keep'
            touch()
          }}
        >
          {label}
        </button>
      {/each}
    </div>
  </div>

  {#if session.settings.material.specularMode === 'set'}
    <div class="field">
      <label for="spec-factor">
        specularFactor <span class="num">{session.settings.material.specularFactor.toFixed(2)}</span>
      </label>
      <div class="field-inline">
        <input
          id="spec-factor"
          type="range"
          min="0"
          max="1"
          step="0.01"
          bind:value={session.settings.material.specularFactor}
          oninput={touch}
        />
        <input
          class="num box"
          type="number"
          min="0"
          max="1"
          step="0.05"
          bind:value={session.settings.material.specularFactor}
          oninput={touch}
        />
      </div>
    </div>
  {/if}

  <p class="hint">
    The hand-processed monsters — hobgoblin, gnoll, ogre, troll — carry no specular extension at all,
    which is what <em>Remove</em> reproduces. <em>Set</em> writes a plain <code>specularFactor</code> and
    drops the colour tint, the way stone_golem ships a deliberate 0.30. <em>Keep</em> leaves the 2×
    the importer applied, which is only useful for seeing what it was doing.
  </p>
</section>

<section class="panel">
  <h3>Metallic-roughness map</h3>
  <label class="field-inline">
    <input type="checkbox" bind:checked={session.settings.deriveMetallicRoughness} onchange={touch} />
    Derive one from the albedo
  </label>
  {#if session.settings.deriveMetallicRoughness}
    <div class="field">
      <label for="mr-sat">Saturation ceiling <span class="num">{session.settings.mrParams.saturationCeiling.toFixed(2)}</span></label>
      <input id="mr-sat" type="range" min="0" max="1" step="0.01" bind:value={session.settings.mrParams.saturationCeiling} oninput={touch} />
    </div>
    <div class="field">
      <label for="mr-val">Brightness ceiling <span class="num">{session.settings.mrParams.valueCeiling.toFixed(2)}</span></label>
      <input id="mr-val" type="range" min="0" max="1" step="0.01" bind:value={session.settings.mrParams.valueCeiling} oninput={touch} />
    </div>
    <div class="grid2">
      <div class="field">
        <label for="mr-metal">Metal value</label>
        <input id="mr-metal" class="num" type="number" min="0" max="1" step="0.05" bind:value={session.settings.mrParams.metallic} oninput={touch} />
      </div>
      <div class="field">
        <label for="mr-mrough">Metal roughness</label>
        <input id="mr-mrough" class="num" type="number" min="0" max="1" step="0.02" bind:value={session.settings.mrParams.metalRoughness} oninput={touch} />
      </div>
    </div>
    {#if session.result?.metalFraction !== null && session.result?.metalFraction !== undefined}
      <p class="hint">
        <span class="num">{(session.result.metalFraction * 100).toFixed(1)}%</span> of the texture is being
        read as bare metal.
      </p>
    {/if}
    <p class="hint">
      Meshy only ships base colour, so this infers the map: dark and colourless reads as plate. It is a
      guess, and it misreads dark hair and claws — troll ships with flat factors for exactly that reason.
      Look at the result under <em>Game · noon</em> before keeping it.
    </p>
  {/if}
</section>

<section class="panel">
  <h3>Textures</h3>
  <div class="grid2">
    <div class="field">
      <label for="tex-size">Max size</label>
      <select id="tex-size" bind:value={session.settings.texture.maxSize} onchange={touch}>
        <option value={2048}>2048²</option>
        <option value={1024}>1024² (project standard)</option>
        <option value={512}>512²</option>
      </select>
    </div>
    <div class="field">
      <label for="tex-q">JPEG quality <span class="num">{session.settings.texture.quality.toFixed(2)}</span></label>
      <input id="tex-q" type="range" min="0.5" max="1" step="0.01" bind:value={session.settings.texture.quality} onchange={touch} />
    </div>
  </div>

  {#if session.result}
    <dl class="stats">
      {#each session.result.stats.images as image (image.index)}
        <dt>{image.name}</dt>
        <dd>{image.width}×{image.height} · {(image.byteLength / 1024).toFixed(0)} KB</dd>
      {/each}
      <dt>GLB total</dt>
      <dd>{(session.result.stats.byteLength / 1_000_000).toFixed(2)} MB</dd>
    </dl>
  {/if}
  {#each session.result?.textureChanges ?? [] as change (change.index)}
    <p class="hint">
      {change.name}: {change.from.width}×{change.from.height} → {change.to.width}×{change.to.height},
      {(change.from.bytes / 1024).toFixed(0)} KB → {(change.to.bytes / 1024).toFixed(0)} KB
    </p>
  {/each}
  <p class="hint">
    Textures are most of a model's bytes and every player downloads them. The project standard is 1024²
    JPEG at 0.88.
  </p>
</section>

{#if after.length > 0}
  <section class="panel">
    <h3>After</h3>
    {#each after as material (material.index)}
      <dl class="stats">
        <dt>{material.name}</dt>
        <dd></dd>
        <dt>metallic / roughness</dt>
        <dd>{material.metallicFactor.toFixed(2)} / {material.roughnessFactor.toFixed(2)}</dd>
        <dt>emissive</dt>
        <dd>{material.hasEmissiveTexture ? 'texture' : material.emissive.map((c) => c.toFixed(2)).join(' ')}</dd>
        <dt>mr texture</dt>
        <dd>{material.hasMetallicRoughnessTexture ? 'yes' : 'no'}</dd>
        <dt>specular</dt>
        <dd>{material.specular === null ? '—' : material.specular.toFixed(2)}</dd>
      </dl>
    {/each}
  </section>
{/if}

<style>
  .bad {
    color: var(--red);
  }

  .box {
    width: 78px;
    text-align: right;
  }
</style>
