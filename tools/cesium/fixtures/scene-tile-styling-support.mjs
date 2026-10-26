import { suite } from "./lib.mjs";
import Cesium3DTilesetStatistics from "../node_modules/@cesium/engine/Source/Scene/Cesium3DTilesetStatistics.js";
import Model3DTileContent from "../node_modules/@cesium/engine/Source/Scene/Model/Model3DTileContent.js";
import Cesium3DTilesetHeatmap from "../node_modules/@cesium/engine/Source/Scene/Cesium3DTilesetHeatmap.js";
import Cesium3DTileOptimizations from "../node_modules/@cesium/engine/Source/Scene/Cesium3DTileOptimizations.js";
import Cesium3DTileOptimizationHint from "../node_modules/@cesium/engine/Source/Scene/Cesium3DTileOptimizationHint.js";
import TileOrientedBoundingBox from "../node_modules/@cesium/engine/Source/Scene/TileOrientedBoundingBox.js";
import Cartesian3 from "../node_modules/@cesium/engine/Source/Core/Cartesian3.js";
import Matrix3 from "../node_modules/@cesium/engine/Source/Core/Matrix3.js";

export default function sceneTileStylingSupport() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };

  // ---- Cesium3DTilesetStatistics ----
  {
    const s = add("Cesium3DTilesetStatistics");

    s.add("basic_counts", [], () => {
      const stats = new Cesium3DTilesetStatistics();
      stats.selected = 12;
      stats.visited = 34;
      stats.numberOfCommands = 56;
      stats.numberOfAttemptedRequests = 7;
      stats.numberOfPendingRequests = 3;
      stats.numberOfTilesProcessing = 2;
      stats.numberOfTilesWithContentReady = 10;
      stats.numberOfTilesTotal = 100;
      stats.numberOfLoadedTilesTotal = 15;
      stats.numberOfTilesCulledWithChildrenUnion = 4;

      const cloned = new Cesium3DTilesetStatistics();
      Cesium3DTilesetStatistics.clone(stats, cloned);

      stats.clear();
      return {
        cleared: {
          selected: stats.selected,
          visited: stats.visited,
          numberOfCommands: stats.numberOfCommands,
          numberOfAttemptedRequests: stats.numberOfAttemptedRequests,
          numberOfPendingRequests: stats.numberOfPendingRequests,
          numberOfTilesProcessing: stats.numberOfTilesProcessing,
          numberOfTilesWithContentReady: stats.numberOfTilesWithContentReady,
          numberOfTilesTotal: stats.numberOfTilesTotal,
          numberOfLoadedTilesTotal: stats.numberOfLoadedTilesTotal,
          numberOfTilesCulledWithChildrenUnion: stats.numberOfTilesCulledWithChildrenUnion,
        },
        cloned: {
          selected: cloned.selected,
          visited: cloned.visited,
          numberOfCommands: cloned.numberOfCommands,
          numberOfAttemptedRequests: cloned.numberOfAttemptedRequests,
          numberOfPendingRequests: cloned.numberOfPendingRequests,
          numberOfTilesProcessing: cloned.numberOfTilesProcessing,
          numberOfTilesWithContentReady: cloned.numberOfTilesWithContentReady,
          numberOfTilesTotal: cloned.numberOfTilesTotal,
          numberOfLoadedTilesTotal: cloned.numberOfLoadedTilesTotal,
          numberOfTilesCulledWithChildrenUnion: cloned.numberOfTilesCulledWithChildrenUnion,
        },
      };
    });

    s.add("content_selection_and_loads", [], () => {
      const stats = new Cesium3DTilesetStatistics();

      const leafContent1 = {
        featuresLength: 10,
        pointsLength: 100,
        trianglesLength: 50,
        geometryByteLength: 2048,
        batchTableByteLength: 512,
        texturesByteLength: 4096,
        innerContents: undefined,
      };

      const leafContent2 = {
        featuresLength: 5,
        pointsLength: 20,
        trianglesLength: 10,
        geometryByteLength: 1024,
        batchTableByteLength: 256,
        texturesByteLength: 2048,
        innerContents: undefined,
      };

      const compositeContent = {
        featuresLength: 0,
        pointsLength: 0,
        trianglesLength: 0,
        geometryByteLength: 0,
        batchTableByteLength: 0,
        texturesByteLength: 0,
        innerContents: [leafContent1, leafContent2],
      };

      stats.incrementSelectionCounts(compositeContent);
      const afterSelect = {
        numberOfFeaturesSelected: stats.numberOfFeaturesSelected,
        numberOfPointsSelected: stats.numberOfPointsSelected,
        numberOfTrianglesSelected: stats.numberOfTrianglesSelected,
      };

      stats.incrementLoadCounts(compositeContent);
      const afterLoad = {
        numberOfFeaturesLoaded: stats.numberOfFeaturesLoaded,
        numberOfPointsLoaded: stats.numberOfPointsLoaded,
        geometryByteLength: stats.geometryByteLength,
        batchTableByteLength: stats.batchTableByteLength,
        texturesByteLength: stats.texturesByteLength,
      };

      stats.decrementLoadCounts(leafContent1);
      const afterDecrement = {
        numberOfFeaturesLoaded: stats.numberOfFeaturesLoaded,
        numberOfPointsLoaded: stats.numberOfPointsLoaded,
        geometryByteLength: stats.geometryByteLength,
        batchTableByteLength: stats.batchTableByteLength,
        texturesByteLength: stats.texturesByteLength,
      };

      return { afterSelect, afterLoad, afterDecrement };
    });

    s.add("model_texture_reference_counting", [], () => {
      const stats = new Cesium3DTilesetStatistics();

      const model1 = Object.create(Model3DTileContent.prototype, {
        featuresLength: { value: 20 },
        pointsLength: { value: 0 },
        trianglesLength: { value: 100 },
        geometryByteLength: { value: 4000 },
        batchTableByteLength: { value: 200 },
        texturesByteLength: { value: 0 },
        innerContents: { value: undefined },
        getTextureIds: { value: () => ["texA", "texB"] },
        getTextureByteLengthById: { value: (id) => (id === "texA" ? 1000 : 2000) },
      });

      const model2 = Object.create(Model3DTileContent.prototype, {
        featuresLength: { value: 30 },
        pointsLength: { value: 0 },
        trianglesLength: { value: 150 },
        geometryByteLength: { value: 6000 },
        batchTableByteLength: { value: 300 },
        texturesByteLength: { value: 0 },
        innerContents: { value: undefined },
        getTextureIds: { value: () => ["texB", "texC"] },
        getTextureByteLengthById: { value: (id) => (id === "texB" ? 2000 : 4000) },
      });

      stats.incrementLoadCounts(model1);
      const step1 = {
        texturesByteLength: stats.texturesByteLength,
        refCounts: { ...stats.texturesReferenceCounterById },
      };

      stats.incrementLoadCounts(model2);
      const step2 = {
        texturesByteLength: stats.texturesByteLength,
        refCounts: { ...stats.texturesReferenceCounterById },
      };

      stats.decrementLoadCounts(model1);
      const step3 = {
        texturesByteLength: stats.texturesByteLength,
        refCounts: { ...stats.texturesReferenceCounterById },
      };

      stats.decrementLoadCounts(model2);
      const step4 = {
        texturesByteLength: stats.texturesByteLength,
        refCounts: { ...stats.texturesReferenceCounterById },
      };

      return { step1, step2, step3, step4 };
    });
  }

  // ---- Cesium3DTilesetHeatmap ----
  {
    const s = add("Cesium3DTilesetHeatmap");

    s.add("heatmap_color_palette_interpolation", [
      [0.0, 10.0, 20.0, 35.0, 50.0, 70.0, 80.0, 95.0, 100.0, -10.0, 150.0],
    ], (testValues) => {
      const heatmap = new Cesium3DTilesetHeatmap("error");
      heatmap._previousMinimum = 0.0;
      heatmap._previousMaximum = 100.0;

      const frameState = { frameNumber: 1 };
      const colors = testValues.map((val) => {
        const tile = { error: val, contentAvailable: true, _selectedFrame: 1, _debugColor: undefined };
        heatmap.colorize(tile, frameState);
        return {
          val,
          color: tile._debugColor ? [tile._debugColor.red, tile._debugColor.green, tile._debugColor.blue, tile._debugColor.alpha] : null,
        };
      });

      return { colors };
    });

    s.add("heatmap_lifecycle_min_max_reset", [], () => {
      const heatmap = new Cesium3DTilesetHeatmap("error");
      const frame1 = { frameNumber: 1 };
      const frame2 = { frameNumber: 2 };

      const t1 = { error: 20, contentAvailable: true, _selectedFrame: 1 };
      const t2 = { error: 80, contentAvailable: true, _selectedFrame: 1 };
      heatmap.colorize(t1, frame1);
      heatmap.colorize(t2, frame1);

      const frame1MinMax = { min: heatmap._minimum, max: heatmap._maximum };
      heatmap.resetMinimumMaximum();
      const afterReset = {
        prevMin: heatmap._previousMinimum,
        prevMax: heatmap._previousMaximum,
        min: heatmap._minimum,
        max: heatmap._maximum,
      };

      const t3 = { error: 50, contentAvailable: true, _selectedFrame: 2 };
      heatmap.colorize(t3, frame2);
      const frame2Color = [t3._debugColor.red, t3._debugColor.green, t3._debugColor.blue, t3._debugColor.alpha];

      heatmap.setReferenceMinimumMaximum(10, 200, "error");
      heatmap.resetMinimumMaximum();
      const afterReference = {
        prevMin: heatmap._previousMinimum,
        prevMax: heatmap._previousMaximum,
      };

      return { frame1MinMax, afterReset, frame2Color, afterReference };
    });
  }

  // ---- Cesium3DTileOptimizations ----
  {
    const s = add("Cesium3DTileOptimizations");

    s.add("check_children_within_parent_cases", [], () => {
      const parentBox = new TileOrientedBoundingBox(
        new Cartesian3(0, 0, 0),
        Matrix3.fromScale(new Cartesian3(10, 10, 10))
      );

      const childBoxInside = new TileOrientedBoundingBox(
        new Cartesian3(1, 2, 3),
        Matrix3.fromScale(new Cartesian3(2, 2, 2))
      );

      const childBoxOutside = new TileOrientedBoundingBox(
        new Cartesian3(8, 0, 0),
        Matrix3.fromScale(new Cartesian3(5, 5, 5))
      );

      const tileInside = {
        boundingVolume: parentBox,
        children: [{ boundingVolume: childBoxInside }],
        _optimChildrenWithinParent: Cesium3DTileOptimizationHint.NOT_COMPUTED,
      };
      const resInside = Cesium3DTileOptimizations.checkChildrenWithinParent(tileInside);

      const tileOutside = {
        boundingVolume: parentBox,
        children: [{ boundingVolume: childBoxOutside }],
        _optimChildrenWithinParent: Cesium3DTileOptimizationHint.NOT_COMPUTED,
      };
      const resOutside = Cesium3DTileOptimizations.checkChildrenWithinParent(tileOutside);

      const tileMissingChildObb = {
        boundingVolume: parentBox,
        children: [{ boundingVolume: {} }],
        _optimChildrenWithinParent: Cesium3DTileOptimizationHint.NOT_COMPUTED,
      };
      const resMissingChild = Cesium3DTileOptimizations.checkChildrenWithinParent(tileMissingChildObb);

      const tileMissingParentObb = {
        boundingVolume: {},
        children: [{ boundingVolume: childBoxInside }],
        _optimChildrenWithinParent: Cesium3DTileOptimizationHint.NOT_COMPUTED,
      };
      const resMissingParent = Cesium3DTileOptimizations.checkChildrenWithinParent(tileMissingParentObb);

      return {
        resInside: { result: resInside, hint: tileInside._optimChildrenWithinParent },
        resOutside: { result: resOutside, hint: tileOutside._optimChildrenWithinParent },
        resMissingChild: { result: resMissingChild, hint: tileMissingChildObb._optimChildrenWithinParent },
        resMissingParent: { result: resMissingParent, hint: tileMissingParentObb._optimChildrenWithinParent },
      };
    });
  }

  return out;
}
