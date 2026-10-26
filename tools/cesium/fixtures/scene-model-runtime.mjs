import ModelArticulationStage from "@cesium/engine/Source/Scene/Model/ModelArticulationStage.js";
import ModelArticulation from "@cesium/engine/Source/Scene/Model/ModelArticulation.js";
import ModelAnimationChannel from "@cesium/engine/Source/Scene/Model/ModelAnimationChannel.js";
import ModelMatrixUpdateStage from "@cesium/engine/Source/Scene/Model/ModelMatrixUpdateStage.js";
import MappedPositions from "@cesium/engine/Source/Scene/Model/MappedPositions.js";
import ModelRuntimePrimitive from "@cesium/engine/Source/Scene/Model/ModelRuntimePrimitive.js";
import ModelRuntimeNode from "@cesium/engine/Source/Scene/Model/ModelRuntimeNode.js";
import ArticulationStageType from "@cesium/engine/Source/Core/ArticulationStageType.js";
import InterpolationType from "@cesium/engine/Source/Core/InterpolationType.js";
import ModelComponents from "@cesium/engine/Source/Scene/ModelComponents.js";
import SceneMode from "@cesium/engine/Source/Scene/SceneMode.js";
import PrimitiveType from "@cesium/engine/Source/Core/PrimitiveType.js";
import {
  Cartesian3,
  Cartographic,
  Ellipsoid,
  Matrix4,
  Quaternion,
  Rectangle,
} from "@cesium/engine";
import { suite, enc } from "./lib.mjs";

const { AnimatedPropertyType } = ModelComponents;

export function sceneModelRuntime() {
  const suites = [];

  // 1. ModelArticulationStage
  const stageSuite = suite("ModelArticulationStage");
  const stageTypes = [
    ArticulationStageType.XROTATE,
    ArticulationStageType.YROTATE,
    ArticulationStageType.ZROTATE,
    ArticulationStageType.XTRANSLATE,
    ArticulationStageType.YTRANSLATE,
    ArticulationStageType.ZTRANSLATE,
    ArticulationStageType.XSCALE,
    ArticulationStageType.YSCALE,
    ArticulationStageType.ZSCALE,
    ArticulationStageType.UNIFORMSCALE,
  ];

  for (const t of stageTypes) {
    stageSuite.add("apply", [t, 2.5, enc(Matrix4.IDENTITY)], (type, val) => {
      const mockArticulation = { _dirty: false };
      const s = new ModelArticulationStage({
        stage: {
          name: type,
          type: type,
          minimumValue: -100,
          maximumValue: 100,
          initialValue: val,
        },
        runtimeArticulation: mockArticulation,
      });
      return s.applyStageToMatrix(Matrix4.clone(Matrix4.IDENTITY));
    });
  }

  // Chained stages
  stageSuite.add(
    "chained",
    [
      [
        { type: ArticulationStageType.XTRANSLATE, val: 10 },
        { type: ArticulationStageType.ZROTATE, val: 90 },
        { type: ArticulationStageType.UNIFORMSCALE, val: 2 },
      ],
    ],
    (stages) => {
      let m = Matrix4.clone(Matrix4.IDENTITY);
      const mockArticulation = { _dirty: false };
      for (const st of stages) {
        const s = new ModelArticulationStage({
          stage: {
            name: st.type,
            type: st.type,
            minimumValue: -1000,
            maximumValue: 1000,
            initialValue: st.val,
          },
          runtimeArticulation: mockArticulation,
        });
        m = s.applyStageToMatrix(m);
      }
      return m;
    },
  );

  // Clamping and epsilon
  stageSuite.add(
    "clamping",
    [-10, 10, -20, 5, 20],
    (min, max, under, inside, over) => {
      const mockArticulation = { _dirty: false };
      const s = new ModelArticulationStage({
        stage: {
          name: "clampTest",
          type: ArticulationStageType.XTRANSLATE,
          minimumValue: min,
          maximumValue: max,
          initialValue: 0,
        },
        runtimeArticulation: mockArticulation,
      });
      s.currentValue = under;
      const vUnder = s.currentValue;
      s.currentValue = inside;
      const vInside = s.currentValue;
      s.currentValue = over;
      const vOver = s.currentValue;
      return [vUnder, vInside, vOver];
    },
  );
  suites.push(stageSuite);

  // 2. ModelArticulation
  const artSuite = suite("ModelArticulation");
  artSuite.add(
    "applyArticulation",
    [
      [
        {
          name: "yaw",
          type: ArticulationStageType.ZROTATE,
          min: -180,
          max: 180,
          val: 90,
        },
        {
          name: "pitch",
          type: ArticulationStageType.XROTATE,
          min: -90,
          max: 90,
          val: 45,
        },
        {
          name: "dist",
          type: ArticulationStageType.YTRANSLATE,
          min: -100,
          max: 100,
          val: 5,
        },
      ],
      [
        enc(Matrix4.fromTranslation(new Cartesian3(1, 2, 3))),
        enc(Matrix4.fromScale(new Cartesian3(2, 2, 2))),
      ],
    ],
    (stagesDesc, nodeMatrices) => {
      const articulation = {
        name: "testArticulation",
        stages: stagesDesc.map((s) => ({
          name: s.name,
          type: s.type,
          minimumValue: s.min,
          maximumValue: s.max,
          initialValue: 0,
        })),
      };
      const runtime = new ModelArticulation({
        articulation,
        sceneGraph: {},
      });
      const nodes = nodeMatrices.map((m, i) => {
        const mat = Matrix4.fromArray(m);
        return new ModelRuntimeNode({
          node: { index: i },
          transform: mat,
          transformToRoot: Matrix4.IDENTITY,
          children: [],
          sceneGraph: {},
        });
      });
      for (const n of nodes) {
        runtime._runtimeNodes.push(n);
      }
      for (const s of stagesDesc) {
        runtime.setArticulationStage(s.name, s.val);
      }
      runtime.apply();
      return nodes.map((n) => n.transform);
    },
  );
  suites.push(artSuite);

  // 3. ModelAnimationChannel
  const chanSuite = suite("ModelAnimationChannel");

  // Translation channel
  chanSuite.add("animateTranslation", [0.0, 0.5, 1.0, 1.5, 2.0], (...times) => {
    const tSampler = {
      input: [0, 1, 2],
      output: [
        new Cartesian3(0, 0, 0),
        new Cartesian3(10, 20, 30),
        new Cartesian3(20, 40, 60),
      ],
      interpolation: InterpolationType.LINEAR,
    };
    const tChannel = new ModelAnimationChannel({
      channel: {
        sampler: tSampler,
        target: { node: 0, path: AnimatedPropertyType.TRANSLATION },
      },
      runtimeAnimation: { model: { clampAnimations: false } },
      runtimeNode: { translation: new Cartesian3(), userAnimated: false },
    });
    return times.map((t) => {
      tChannel.animate(t);
      return Cartesian3.clone(tChannel._runtimeNode.translation);
    });
  });

  // Rotation channel (quaternion)
  chanSuite.add("animateRotation", [0.0, 0.25, 0.5, 0.75, 1.0], (...times) => {
    const q0 = Quaternion.IDENTITY;
    const q1 = Quaternion.fromAxisAngle(Cartesian3.UNIT_Z, Math.PI / 2);
    const rSampler = {
      input: [0, 1],
      output: [q0, q1],
      interpolation: InterpolationType.LINEAR,
    };
    const rChannel = new ModelAnimationChannel({
      channel: {
        sampler: rSampler,
        target: { node: 0, path: AnimatedPropertyType.ROTATION },
      },
      runtimeAnimation: { model: { clampAnimations: false } },
      runtimeNode: { rotation: new Quaternion(), userAnimated: false },
    });
    return times.map((t) => {
      rChannel.animate(t);
      return Quaternion.clone(rChannel._runtimeNode.rotation);
    });
  });

  // Scale channel (step)
  chanSuite.add("animateScale", [0.0, 0.4, 1.0, 1.5, 2.0], (...times) => {
    const sSampler = {
      input: [0, 1, 2],
      output: [
        new Cartesian3(1, 1, 1),
        new Cartesian3(2, 2, 2),
        new Cartesian3(4, 4, 4),
      ],
      interpolation: InterpolationType.STEP,
    };
    const sChannel = new ModelAnimationChannel({
      channel: {
        sampler: sSampler,
        target: { node: 0, path: AnimatedPropertyType.SCALE },
      },
      runtimeAnimation: { model: { clampAnimations: false } },
      runtimeNode: { scale: new Cartesian3(), userAnimated: false },
    });
    return times.map((t) => {
      sChannel.animate(t);
      return Cartesian3.clone(sChannel._runtimeNode.scale);
    });
  });

  // Weights channel (morph targets cubic)
  chanSuite.add("animateWeights", [0.0, 0.25, 0.5, 0.75, 1.0], (...times) => {
    const wSampler = {
      input: [0, 1],
      output: [0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0],
      interpolation: InterpolationType.CUBICSPLINE,
    };
    const wChannel = new ModelAnimationChannel({
      channel: {
        sampler: wSampler,
        target: { node: 0, path: AnimatedPropertyType.WEIGHTS },
      },
      runtimeAnimation: { model: { clampAnimations: false } },
      runtimeNode: { morphWeights: [0, 0], userAnimated: false },
    });
    return times.map((t) => {
      wChannel.animate(t);
      return Array.from(wChannel._runtimeNode.morphWeights);
    });
  });
  suites.push(chanSuite);

  // 4. ModelMatrixUpdateStage
  const updateSuite = suite("ModelMatrixUpdateStage");
  updateSuite.add(
    "updateHierarchy",
    [
      enc(Matrix4.fromTranslation(new Cartesian3(1, 0, 0))),
      enc(Matrix4.fromTranslation(new Cartesian3(0, 2, 0))),
      enc(Matrix4.fromTranslation(new Cartesian3(0, 0, 3))),
      enc(Matrix4.fromTranslation(new Cartesian3(10, 0, 0))),
    ],
    (rootT, childT, grandT, modelT) => {
      const rootNode = new ModelRuntimeNode({
        node: { index: 0 },
        transform: Matrix4.fromArray(rootT),
        transformToRoot: Matrix4.IDENTITY,
        children: [1],
        sceneGraph: {},
      });
      rootNode._transformDirty = true;

      const childNode = new ModelRuntimeNode({
        node: { index: 1 },
        transform: Matrix4.fromArray(childT),
        transformToRoot: Matrix4.IDENTITY,
        children: [2],
        sceneGraph: {},
      });

      const grandChild = new ModelRuntimeNode({
        node: { index: 2 },
        transform: Matrix4.fromArray(grandT),
        transformToRoot: Matrix4.IDENTITY,
        children: [],
        sceneGraph: {},
      });
      grandChild.runtimePrimitives.push({
        drawCommand: {
          modelMatrix: new Matrix4(),
          primitiveType: 4,
          cullFace: 0,
        },
      });

      const sceneGraph = {
        _model: { _projectTo2D: false },
        _computedModelMatrix: Matrix4.fromArray(modelT),
        _computedModelMatrix2D: Matrix4.IDENTITY,
        _runtimeNodes: [rootNode, childNode, grandChild],
      };

      const frameState = { mode: SceneMode.SCENE3D };
      ModelMatrixUpdateStage.update(rootNode, sceneGraph, frameState);

      return {
        rootComputed: rootNode.computedTransform,
        childToRoot: childNode.transformToRoot,
        childComputed: childNode.computedTransform,
        grandToRoot: grandChild.transformToRoot,
        grandComputed: grandChild.computedTransform,
        drawCommandModelMatrix:
          grandChild.runtimePrimitives[0].drawCommand.modelMatrix,
        drawCommandCullFace: grandChild.runtimePrimitives[0].drawCommand.cullFace,
      };
    },
  );
  suites.push(updateSuite);

  // 5. MappedPositions
  const mappedSuite = suite("MappedPositions");
  mappedSuite.add(
    "properties",
    [
      [
        [0.1, 0.2, 10.0],
        [0.3, 0.4, 20.0],
      ],
      [0.1, 0.2, 0.3, 0.4],
    ],
    (coords, rect) => {
      const positions = coords.map((c) => new Cartographic(c[0], c[1], c[2]));
      const r = new Rectangle(rect[0], rect[1], rect[2], rect[3]);
      const mp = new MappedPositions(
        positions,
        positions.length,
        r,
        Ellipsoid.WGS84,
      );
      return {
        numPositions: mp.numPositions,
        rect: [r.west, r.south, r.east, r.north],
        positions: mp.cartographicPositions.map((p) => [
          p.longitude,
          p.latitude,
          p.height,
        ]),
        radii: [
          mp.ellipsoid.radii.x,
          mp.ellipsoid.radii.y,
          mp.ellipsoid.radii.z,
        ],
      };
    },
  );
  suites.push(mappedSuite);

  // 6. ModelRuntimePrimitive
  const primSuite = suite("ModelRuntimePrimitive");
  const testConfigs = [
    { name: "default", use2D: false, vertExag: 1.0, wireframe: false },
    { name: "2D", use2D: true, vertExag: 1.0, wireframe: false },
    { name: "vertExag", use2D: false, vertExag: 2.0, wireframe: false },
    { name: "wireframe", use2D: false, vertExag: 1.0, wireframe: true },
    { name: "2D_and_wireframe", use2D: true, vertExag: 1.0, wireframe: true },
  ];

  for (const cfg of testConfigs) {
    primSuite.add("pipelineStages", [cfg], (c) => {
      const primitive = {
        attributes: [],
        primitiveType: PrimitiveType.TRIANGLES,
        featureIds: [],
      };
      const node = {};
      const model = {
        _projectTo2D: c.use2D,
        hasVerticalExaggeration: c.vertExag !== 1.0,
        debugWireframe: c.wireframe,
        _enableDebugWireframe: c.wireframe,
        _enableShowOutline: false,
        allowPicking: true,
        type: 0,
        featureIdLabel: "featureId_0",
      };
      const frameState = {
        context: { webgl2: true },
        mode: c.use2D ? SceneMode.SCENE2D : SceneMode.SCENE3D,
        scene3DOnly: false,
        verticalExaggeration: c.vertExag,
      };
      const rp = new ModelRuntimePrimitive({ primitive, node, model });
      rp.configurePipeline(frameState);
      return rp.pipelineStages.map((s) => s.name);
    });
  }
  suites.push(primSuite);

  return suites;
}
