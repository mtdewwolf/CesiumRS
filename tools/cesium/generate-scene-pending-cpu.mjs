// Direct reference captures from the pinned @cesium/engine 23.0.0 for the
// CPU/worker pending-validation Scene modules:
// - Empty3DTileContent
// - JobScheduler
// - createTangentSpaceDebugPrimitive
// - GaussianSplatRenderResources
// - GaussianSplatSorter
// - GaussianSplatTextureGenerator
import { writeFileSync } from 'node:fs';

const C = await import('@cesium/engine');
const { default: ContextLimits } = await import('@cesium/engine/Source/Renderer/ContextLimits.js');
ContextLimits._minimumAliasedLineWidth = 1;
ContextLimits._maximumAliasedLineWidth = 1;

const { default: Empty3DTileContent } = await import('@cesium/engine/Source/Scene/Empty3DTileContent.js');
const { default: JobScheduler } = await import('@cesium/engine/Source/Scene/JobScheduler.js');
const { default: JobType } = await import('@cesium/engine/Source/Scene/JobType.js');
const { default: createTangentSpaceDebugPrimitive } = await import('@cesium/engine/Source/Scene/createTangentSpaceDebugPrimitive.js');
const { default: GaussianSplatRenderResources } = await import('@cesium/engine/Source/Scene/GaussianSplatRenderResources.js');
const { default: GaussianSplatSorter } = await import('@cesium/engine/Source/Scene/GaussianSplatSorter.js');
const { default: GaussianSplatTextureGenerator } = await import('@cesium/engine/Source/Scene/GaussianSplatTextureGenerator.js');
const { default: TaskProcessor } = await import('@cesium/engine/Source/Core/TaskProcessor.js');

const attempt = (fn) => {
  try {
    fn();
    return null;
  } catch (e) {
    return e.message;
  }
};

// --- Empty3DTileContent ----------------------------------------------------
const empty3DTileContent = (() => {
  const c = new Empty3DTileContent({ id: 'tileset' }, { id: 'tile' });
  const initial = {
    featuresLength: c.featuresLength,
    pointsLength: c.pointsLength,
    trianglesLength: c.trianglesLength,
    geometryByteLength: c.geometryByteLength,
    texturesByteLength: c.texturesByteLength,
    batchTableByteLength: c.batchTableByteLength,
    innerContents: c.innerContents ?? null,
    ready: c.ready,
    url: c.url ?? null,
    metadata: c.metadata ?? null,
    batchTable: c.batchTable ?? null,
    group: c.group ?? null,
    featurePropertiesDirty: c.featurePropertiesDirty,
    hasProperty: c.hasProperty(0, 'height'),
    getFeature: c.getFeature(0) ?? null,
    pick: c.pick(undefined, undefined, undefined) ?? null,
    isDestroyed: c.isDestroyed(),
  };
  const setMetadataError = attempt(() => { c.metadata = {}; });
  const setGroupError = attempt(() => { c.group = {}; });
  c.featurePropertiesDirty = true;
  const dirtyAfterSet = c.featurePropertiesDirty;
  c.applyDebugSettings(true, C.Color.RED);
  c.applyStyle({});
  c.update({}, {});
  c.destroy();
  return {
    initial,
    setMetadataError,
    setGroupError,
    dirtyAfterSet,
    isDestroyedAfter: c.isDestroyed(),
  };
})();

// --- JobScheduler ----------------------------------------------------------
const jobScheduler = (() => {
  let now = 0;
  const origGetTimestamp = JobScheduler.getTimestamp;
  JobScheduler.getTimestamp = () => now;

  const snapshot = (s) => ({
    totalBudget: s.totalBudget,
    totalUsedThisFrame: s._totalUsedThisFrame,
    executedThisFrame: [...s._executedThisFrame],
    budgets: s._budgets.map((b) => ({
      total: b.total,
      usedThisFrame: b.usedThisFrame,
      stolenFromMeThisFrame: b.stolenFromMeThisFrame,
      starvedThisFrame: b.starvedThisFrame,
      starvedLastFrame: b.starvedLastFrame,
    })),
  });

  const constructorCases = [
    { name: 'default', budgets: null },
    { name: 'custom', budgets: [5, 15, 20] },
    { name: 'zeros', budgets: [0, 0, 0] },
    { name: 'fractional', budgets: [2.5, 3.5, 4.25] },
    { name: 'tooShort', budgets: [10, 20] },
    { name: 'tooLong', budgets: [1, 2, 3, 4] },
    { name: 'empty', budgets: [] },
  ].map(({ name, budgets }) => {
    let s;
    const error = attempt(() => {
      s = budgets === null ? new JobScheduler() : new JobScheduler(budgets);
    });
    return { name, budgets, error, state: s ? snapshot(s) : null };
  });

  const scripts = [
    {
      name: 'stealing-and-starvation',
      budgets: [10, 10, 10],
      ops: [
        ['exec', JobType.TEXTURE, 5],
        ['exec', JobType.TEXTURE, 6],
        // Texture now 11 >= 10, steals from Program (6ms) and marks Texture starved
        ['exec', JobType.TEXTURE, 6],
        // Texture steals again from Program (5ms -> Program stolen = 11 >= 10)
        ['exec', JobType.TEXTURE, 5],
        // Texture steals from Buffer (9ms -> totalUsed = 31 >= 30!)
        ['exec', JobType.TEXTURE, 9],
        // Texture blocked by total budget
        ['exec', JobType.TEXTURE, 2],
        // Program has NOT progressed yet -> guaranteed progress despite total budget exhausted!
        // Program's budget is full (stolen=11>=10), Buffer has 1ms left -> steals from Buffer
        ['exec', JobType.PROGRAM, 4],
        // Program second time -> blocked by total budget and marked starved
        ['exec', JobType.PROGRAM, 1],
        // Buffer has NOT progressed yet -> guaranteed progress!
        // Buffer budget is full (stolen=13>=10), all budgets full -> stolenBudget falls through to budgets[2]!
        ['exec', JobType.BUFFER, 2],
        ['reset'],
        // Frame 2: Texture and Program were starvedLastFrame, Buffer was NOT.
        // Note: _executedThisFrame persists across resetBudgets!
        ['exec', JobType.TEXTURE, 12],
        // Texture exhausted -> skips Texture (full) and Program (starvedLastFrame), steals from Buffer!
        ['exec', JobType.TEXTURE, 5],
        // Exhaust Buffer completely
        ['exec', JobType.BUFFER, 6],
        // Now Texture cannot steal from Program (starvedLastFrame) nor Buffer (full), and already progressed -> returns false WITHOUT marking starvedThisFrame!
        ['exec', JobType.TEXTURE, 1],
        ['reset'],
        // Verify starvedLastFrame reflects Frame 2 accurately
        ['exec', JobType.PROGRAM, 3],
      ],
    },
    {
      name: 'disable-this-frame-guaranteed-progress',
      budgets: [10, 10, 30],
      ops: [
        ['disable'],
        // Even when disabled, each job type gets guaranteed first progress because executedThisFrame[t] is false
        ['exec', JobType.TEXTURE, 4],
        // Second time is blocked and starved
        ['exec', JobType.TEXTURE, 2],
        ['exec', JobType.PROGRAM, 3],
        ['exec', JobType.PROGRAM, 2],
        ['exec', JobType.BUFFER, 5],
        ['exec', JobType.BUFFER, 2],
        ['reset'],
        // Now all executedThisFrame are true -> disable blocks immediately!
        ['disable'],
        ['exec', JobType.TEXTURE, 1],
        ['exec', JobType.PROGRAM, 1],
        ['exec', JobType.BUFFER, 1],
      ],
    },
    {
      name: 'first-progress-all-exhausted-fallthrough',
      budgets: [5, 5, 5],
      ops: [
        // Starve Program and Buffer on frame 1 so they cannot be stolen from on frame 2,
        // while Texture never executes on frame 1 (executedThisFrame[0] remains false)
        ['exec', JobType.PROGRAM, 6],
        ['exec', JobType.PROGRAM, 6],
        ['exec', JobType.BUFFER, 6],
        ['exec', JobType.BUFFER, 6],
        ['reset'],
        // Program & Buffer now have starvedLastFrame=true.
        // Have Program exhaust its own budget + steal all of Texture's budget (5ms)
        ['exec', JobType.PROGRAM, 5],
        ['exec', JobType.PROGRAM, 5],
        // Now Texture has stolenFromMeThisFrame=5 >= 5, Program is full & starvedLastFrame,
        // Buffer has starvedLastFrame=true, AND Texture has NEVER executed (progressThisFrame=false)!
        // JS loop assigns stolenBudget = budgets[2] at end and runs Texture -> charges Buffer.stolenFromMeThisFrame!
        ['exec', JobType.TEXTURE, 3],
      ],
    },
    {
      name: 'zero-budgets-and-zero-duration',
      budgets: [0, 0, 0],
      ops: [
        ['exec', JobType.TEXTURE, 0],
        ['exec', JobType.TEXTURE, 0],
        ['exec', JobType.PROGRAM, 1.5],
        ['exec', JobType.BUFFER, 0.25],
        ['reset'],
        ['exec', JobType.TEXTURE, 1],
      ],
    },
  ];

  const traces = scripts.map((script) => {
    now = 100.0;
    const s = new JobScheduler(script.budgets);
    const steps = [];
    for (const op of script.ops) {
      let ran = null;
      let invoked = false;
      if (op[0] === 'exec') {
        const [, jobType, duration] = op;
        ran = s.execute({ execute() { invoked = true; now += duration; } }, jobType);
      } else if (op[0] === 'reset') {
        s.resetBudgets();
      } else if (op[0] === 'disable') {
        s.disableThisFrame();
      }
      steps.push({ op, ran, invoked, state: snapshot(s) });
    }
    return { name: script.name, budgets: script.budgets, steps };
  });

  JobScheduler.getTimestamp = origGetTimestamp;
  return { constructorCases, traces };
})();

// --- createTangentSpaceDebugPrimitive --------------------------------------
const tangentSpaceDebugPrimitive = (() => {
  const positions = [0, 0, 0, 10, 0, 0, 0, 10, 0];
  const normals = [0, 0, 1, 0, 0, 1, 0, 0, 1];
  const tangents = [1, 0, 0, 0.6, 0.8, 0, 1, 0, 0];
  const bitangents = [0, 1, 0, -0.8, 0.6, 0, 0, 1, 0];
  const makeGeometry = (which) => {
    const attributes = {
      position: new C.GeometryAttribute({
        componentDatatype: C.ComponentDatatype.DOUBLE,
        componentsPerAttribute: 3,
        values: new Float64Array(positions),
      }),
    };
    if (which.includes('normal')) {
      attributes.normal = new C.GeometryAttribute({
        componentDatatype: C.ComponentDatatype.FLOAT,
        componentsPerAttribute: 3,
        values: new Float32Array(normals),
      });
    }
    if (which.includes('tangent')) {
      attributes.tangent = new C.GeometryAttribute({
        componentDatatype: C.ComponentDatatype.FLOAT,
        componentsPerAttribute: 3,
        values: new Float32Array(tangents),
      });
    }
    if (which.includes('bitangent')) {
      attributes.bitangent = new C.GeometryAttribute({
        componentDatatype: C.ComponentDatatype.FLOAT,
        componentsPerAttribute: 3,
        values: new Float32Array(bitangents),
      });
    }
    return new C.Geometry({ attributes, primitiveType: C.PrimitiveType.TRIANGLES });
  };

  const customMatrix = C.Matrix4.fromTranslationQuaternionRotationScale(
    new C.Cartesian3(100, -50, 25),
    C.Quaternion.fromAxisAngle(C.Cartesian3.UNIT_Z, 0.5),
    new C.Cartesian3(2, 3, 4),
  );

  const specs = [
    { name: 'none', attrs: [], length: null, matrix: null },
    { name: 'normal-default', attrs: ['normal'], length: null, matrix: null },
    { name: 'tangent-custom-len', attrs: ['tangent'], length: 25.5, matrix: null },
    { name: 'bitangent-negative-len', attrs: ['bitangent'], length: -7.25, matrix: null },
    { name: 'all-three-transformed', attrs: ['normal', 'tangent', 'bitangent'], length: 4.0, matrix: C.Matrix4.pack(customMatrix, []) },
  ];

  const cases = specs.map((spec) => {
    const opts = { geometry: makeGeometry(spec.attrs) };
    if (spec.length !== null) opts.length = spec.length;
    if (spec.matrix !== null) opts.modelMatrix = C.Matrix4.unpack(spec.matrix);
    const prim = createTangentSpaceDebugPrimitive(opts);
    if (!prim) {
      return { ...spec, result: null };
    }
    return {
      ...spec,
      result: {
        asynchronous: prim.asynchronous,
        appearance: {
          flat: prim.appearance.flat,
          translucent: prim.appearance.translucent,
          vertexShaderSource: prim.appearance.vertexShaderSource,
          fragmentShaderSource: prim.appearance.fragmentShaderSource,
        },
        instances: prim.geometryInstances.map((inst) => ({
          modelMatrix: C.Matrix4.pack(inst.modelMatrix, []),
          color: {
            componentDatatype: inst.attributes.color.componentDatatype,
            componentsPerAttribute: inst.attributes.color.componentsPerAttribute,
            normalize: inst.attributes.color.normalize,
            value: Array.from(inst.attributes.color.value),
          },
          primitiveType: inst.geometry.primitiveType,
          positions: Array.from(inst.geometry.attributes.position.values),
          boundingSphere: inst.geometry.boundingSphere
            ? {
                center: [
                  inst.geometry.boundingSphere.center.x,
                  inst.geometry.boundingSphere.center.y,
                  inst.geometry.boundingSphere.center.z,
                ],
                radius: inst.geometry.boundingSphere.radius,
              }
            : null,
        })),
      },
    };
  });

  // Also test uncomputed BoxGeometry factory path
  const box = C.BoxGeometry.fromDimensions({
    dimensions: new C.Cartesian3(2, 4, 6),
    vertexFormat: C.VertexFormat.POSITION_AND_NORMAL,
  });
  const boxPrim = createTangentSpaceDebugPrimitive({ geometry: box, length: 3.0 });
  const factoryCase = {
    dimensions: [2, 4, 6],
    length: 3.0,
    instanceCount: boxPrim.geometryInstances.length,
    asynchronous: boxPrim.asynchronous,
    color: Array.from(boxPrim.geometryInstances[0].attributes.color.value),
    positions: Array.from(boxPrim.geometryInstances[0].geometry.attributes.position.values),
  };

  return { positions, normals, tangents, bitangents, cases, factoryCase };
})();

// --- GaussianSplatRenderResources ------------------------------------------
const gaussianSplatRenderResources = [false, true].map((useLogDepth) => {
  const res = new GaussianSplatRenderResources({ _useLogDepth: useLogDepth });
  return {
    useLogDepth,
    renderStateOptions: res.renderStateOptions,
    alphaOptions: {
      pass: res.alphaOptions.pass ?? null,
      alphaMode: res.alphaOptions.alphaMode ?? null,
      alphaCutoff: res.alphaOptions.alphaCutoff ?? null,
    },
    hasSkipLevelOfDetail: res.hasSkipLevelOfDetail,
    uniformKeys: Object.keys(res.uniformMap),
    vertexDefines: [...res.shaderBuilder._vertexShaderParts.defineLines],
    fragmentDefines: [...res.shaderBuilder._fragmentShaderParts.defineLines],
  };
});

// --- GaussianSplatSorter & GaussianSplatTextureGenerator -------------------
const gaussianCoordinators = await (async () => {
  const concurrencyFormula = [0, 1, 2, 3, 4, 8, 16].map((hc) => ({
    hardwareConcurrency: hc,
    maxSortingConcurrency: Math.max(hc - 1, 1),
  }));

  async function captureCoordinator(Cls, methodName, sampleParams) {
    const origInit = TaskProcessor.prototype.initWebAssemblyModule;
    const origSchedule = TaskProcessor.prototype.scheduleTask;
    const scenarios = [];

    for (const mode of ['success', 'falseInit', 'rejectInit']) {
      Cls._sorterTaskProcessor = undefined;
      Cls._textureTaskProcessor = undefined;
      Cls._taskProcessorReady = false;
      Cls._error = undefined;

      let initCalls = [];
      let scheduledCalls = [];
      let resolveInit, rejectInit;
      TaskProcessor.prototype.initWebAssemblyModule = function (opts) {
        initCalls.push({ workerName: this._workerPath, maxTasks: this._maximumActiveTasks, opts });
        return new Promise((res, rej) => {
          resolveInit = res;
          rejectInit = rej;
        });
      };
      TaskProcessor.prototype.scheduleTask = function (params, transfer) {
        scheduledCalls.push({ transferCount: transfer ? transfer.length : 0 });
        return Promise.resolve('scheduled-ok');
      };

      const beforeInit = Cls[methodName](sampleParams);
      if (mode === 'success') resolveInit(true);
      else if (mode === 'falseInit') resolveInit(false);
      else rejectInit(new Error('wasm load failed'));
      await new Promise((r) => setTimeout(r, 0));

      let afterVal = null;
      let afterErr = null;
      try {
        const p = Cls[methodName](sampleParams);
        afterVal = p ? await p : null;
      } catch (e) {
        afterErr = e.message;
      }
      // Second call after failure must keep the error without re-initializing
      let secondErr = null;
      try {
        Cls[methodName](sampleParams);
      } catch (e) {
        secondErr = e.message;
      }
      scenarios.push({
        mode,
        beforeInit: beforeInit ?? null,
        initCount: initCalls.length,
        workerName: initCalls[0]?.workerName ?? null,
        wasmBinaryFile: initCalls[0]?.opts?.wasmBinaryFile ?? null,
        readyAfter: Cls._taskProcessorReady,
        afterVal,
        afterErr,
        secondErr,
        scheduledCount: scheduledCalls.length,
      });
    }

    TaskProcessor.prototype.initWebAssemblyModule = origInit;
    TaskProcessor.prototype.scheduleTask = origSchedule;
    return scenarios;
  }

  const sorter = await captureCoordinator(GaussianSplatSorter, 'radixSortIndexes', {
    primitive: { positions: new Float32Array([0, 0, 1]), modelView: new Float32Array(16), count: 1 },
    sortType: 'Index',
  });
  const textureGenerator = await captureCoordinator(GaussianSplatTextureGenerator, 'generateFromAttributes', {
    attributes: {
      positions: new Float32Array([0, 0, 0]),
      scales: new Float32Array([1, 1, 1]),
      rotations: new Float32Array([0, 0, 0, 1]),
      colors: new Uint8Array([255, 255, 255, 255]),
    },
    count: 1,
  });

  return { concurrencyFormula, sorter, textureGenerator };
})();

const out = {
  cesium: '23.0.0',
  empty3DTileContent,
  jobScheduler,
  tangentSpaceDebugPrimitive,
  gaussianSplatRenderResources,
  gaussianCoordinators,
};

writeFileSync(new URL('../../tests/data/scene-pending-cpu.json', import.meta.url), JSON.stringify(out, null, 2) + '\n');
console.log('Wrote tests/data/scene-pending-cpu.json');
