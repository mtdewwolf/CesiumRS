// Expression, ConditionsExpression and Cesium3DTileStyle evaluated by the
// pinned CesiumJS engine.
//
// Feature objects are plain objects with the small feature surface the
// expression evaluator uses: inherited property access, class checks and
// tileset time. Rust rebuilds the same mock through `StyleFeature`.
import { suite } from "./lib.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;

const SHADER_MAP = {
  Height: "height",
  id: "id",
  Area: "area",
  Temperature: "temperature",
  red: "red",
  green: "green",
  blue: "blue",
  name: "name",
  a: "a",
  b: "b",
  x: "x",
  y: "y",
  z: "z",
  County: "county",
  YearBuilt: "yearBuilt",
};

function feature(props, className) {
  return {
    getPropertyInherited(name) {
      return props[name];
    },
    isExactClass(name) {
      return className === name;
    },
    isClass(name) {
      return className === name;
    },
    getExactClassName() {
      return className;
    },
    content: { tileset: { timeSinceLoad: 42.5 } },
  };
}

export default async function sceneStyle() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const Expression = await scene("Expression");
  const ConditionsExpression = await scene("ConditionsExpression");
  const Cesium3DTileStyle = await scene("Cesium3DTileStyle");

  const features = [
    { Height: 12, id: "alpha", Area: 80, Temperature: 95, name: "Chest", a: 1.5, b: 2.5 },
    { Height: -4, id: "beta", Area: 3, Temperature: 70, name: "door", a: -2, b: 0.5 },
    {
      Height: 0,
      id: "gamma",
      Area: 0,
      Temperature: 12,
      name: "roof",
      a: 0,
      b: 4,
      nested: { deep: 7, label: "x" },
      list: [10, 20, 30],
      flag: true,
      nothing: null,
    },
  ];

  // ---- Expression evaluation ----
  {
    const s = add("Expression");
    const expressions = [
      "1",
      "1.5",
      "-0",
      "1e3",
      "0.5 + 2.25",
      "1 + 2 * 3",
      "(1 + 2) * 3",
      "true",
      "false",
      "null",
      "undefined",
      "NaN",
      "Infinity",
      "Math.PI",
      "Number.POSITIVE_INFINITY",
      "'hello'",
      '"a" + "b"',
      "'n=' + ${Height}",
      "[1, 2, 3]",
      "${Height}",
      "${missing}",
      "${nested}",
      "${nested.deep}",
      "${list[1]}",
      "${list.length}",
      "${flag}",
      "${nothing}",
      "${Height} > 0",
      "${Height} >= 0 && ${Height} <= 20",
      "${Height} > 0 ? 'up' : 'down'",
      "!(${Height} > 0)",
      "-${Height}",
      "+${Height}",
      "abs(${Height})",
      "sqrt(${Area})",
      "floor(${a})",
      "ceil(${a})",
      "round(${a})",
      "round(2.5)",
      "round(-2.5)",
      "sign(${a})",
      "fract(${a})",
      "min(${a}, ${b})",
      "max(${a}, ${b})",
      "clamp(${a}, 0, 1)",
      "mix(${a}, ${b}, 0.25)",
      "pow(${a}, 2)",
      "atan2(${a}, ${b})",
      "length(vec2(${a}, ${b}))",
      "normalize(vec3(3, 0, 4))",
      "vec2(1, 2)",
      "vec3(1, 2, 3)",
      "vec4(1, 2, 3, 4)",
      "vec3(vec2(1, 2), 3)",
      "vec3(5)",
      "vec2(vec4(1, 2, 3, 4))",
      "vec2(1, 2) + vec2(3, 4)",
      "vec2(1, 2) - vec2(3, 4)",
      "vec2(1, 2) * 2",
      "2 * vec2(1, 2)",
      "vec2(4, 6) / 2",
      "vec2(4, 6) / vec2(2, 3)",
      "vec2(5, 6) % vec2(2, 4)",
      "dot(vec3(1, 2, 3), vec3(4, 5, 6))",
      "cross(vec3(1, 0, 0), vec3(0, 1, 0))",
      "distance(vec3(0, 0, 0), vec3(3, 4, 0))",
      "vec3(1, 2, 3) === vec3(1, 2, 3)",
      "vec3(1, 2, 3) !== vec3(1, 2, 4)",
      "1 === 1",
      "1 !== '1'",
      "Boolean(${Height})",
      "Number('12.5')",
      "Number('x')",
      "String(12.5)",
      "String(vec3(1, 2, 3))",
      "isNaN(${missing})",
      "isFinite(${Height})",
      "color('red')",
      "color('#00ff00')",
      "color('rgba(1, 2, 3, 0.5)')",
      "color('blue', 0.25)",
      "rgb(255, 0, 0)",
      "rgba(0, 255, 0, 0.5)",
      "hsl(0.5, 0.5, 0.5)",
      "hsla(0.25, 0.5, 0.5, 0.5)",
      "color('white').r",
      "color('white')['g']",
      "vec4(1, 2, 3, 4).x",
      "vec4(1, 2, 3, 4)['w']",
      "isExactClass('door')",
      "isClass('door')",
      "getExactClassName()",
      "tiles3d_tileset_time",
      "regExp('^Chest').test(${name})",
      "regExp('^Chest') =~ ${name}",
      "${name} =~ regExp('^Chest')",
      "${name} !~ regExp('^Chest')",
      "regExp('(\\\\d+)').exec('abc123')",
      "regExp('^a', 'i').test('ABC')",
      "regExp('^a').toString()",
      "'Building id ${id} has height ${Height}.'",
      "defined(${Height})",
    ];
    for (const expression of expressions) {
      s.add("evaluate", [expression, features], (src, propsList) => {
        const parsed = new Expression(src);
        return {
          variables: parsed.getVariables(),
          evaluations: propsList.map((props) => parsed.evaluate(feature(props, "door"))),
          shader: (() => {
            const state = { translucent: false };
            try {
              return { expression: parsed.getShaderExpression(SHADER_MAP, state) ?? null, translucent: state.translucent };
            } catch (e) {
              return { throws: e.name, message: e.message, translucent: state.translucent };
            }
          })(),
        };
      });
    }

    // Colors and color shader functions.
    for (const expression of [
      "color('red')",
      "color('#00ff00', 0.5)",
      "rgba(0, 255, 0, 0.5)",
      "hsla(0.25, 0.5, 0.5, 0.25)",
      "(${Temperature} > 90) ? color('red') : color('white')",
    ]) {
      s.add("evaluateColor", [expression, features], (src, propsList) => {
        const parsed = new Expression(src);
        const state = { translucent: false };
        return {
          evaluations: propsList.map((props) => parsed.evaluateColor(feature(props, "door"))),
          shaderFunction: parsed.getShaderFunction("vec4 getColor(float x)", SHADER_MAP, state, "vec4"),
          translucent: state.translucent,
        };
      });
    }

    // Defines.
    s.add("defines", ["${Threshold} * 2", { Threshold: "5" }, features], (src, defines, propsList) => {
      const parsed = new Expression(src, defines);
      return {
        variables: parsed.getVariables(),
        evaluations: propsList.map((props) => parsed.evaluate(feature(props, "door"))),
        shader: parsed.getShaderExpression(SHADER_MAP, { translucent: false }) ?? null,
      };
    });

    // Construction errors and evaluation errors.
    for (const expression of [
      "",
      "1 2",
      "${a} +",
      "unknownIdentifier",
      "Math.sqrt(4)",
      "regExp('[')",
      "vec4(1, 2)",
      "vec3(1, 2, 3, 4)",
      "getExactClassName(1)",
      "isClass()",
      "color()",
      "hsl(1, 2)",
      "1 === ",
      "!1",
      "1 < 'a'",
      "1 && true",
      "color('not a color')",
      "unknown()",
      "foo.test('x')",
      "config.anything",
    ]) {
      s.add("errors", [expression], (src) => {
        const parsed = new Expression(src);
        const state = { translucent: false };
        const value = parsed.evaluate(feature(features[0], "door"));
        const shader = parsed.getShaderExpression(SHADER_MAP, state) ?? null;
        return { value, shader };
      });
    }

    // Variable names and shader sentinels with a partial substitution map.
    for (const expression of [
      "${Height} + ${missing}",
      "${a}.x",
      "${feature}.Height",
      "${feature}['id']",
      "vec3(${red}, ${green}, ${blue})",
    ]) {
      s.add("variables", [expression], (src) => {
        const parsed = new Expression(src);
        return {
          variables: parsed.getVariables(),
          shader: parsed.getShaderExpression({ Height: "height", a: "a", red: "r", green: "g", blue: "b" }, { translucent: false }) ?? null,
        };
      });
    }
  }

  // ---- ConditionsExpression ----
  {
    const s = add("ConditionsExpression");
    const conditionSets = [
      {
        conditions: [
          ["${Height} > 10", "color('red')"],
          ["${Height} > 0", "color('yellow')"],
          ["true", "color('blue')"],
        ],
      },
      { conditions: [["${Area} > 10", "'big'"], ["true", "'small'"]] },
      { conditions: [] },
      {},
    ];
    for (const conditions of conditionSets) {
      s.add("evaluate", [conditions, features], (conditionsJson, propsList) => {
        const parsed = new ConditionsExpression(conditionsJson);
        const state = { translucent: false };
        return {
          variables: parsed.getVariables(),
          evaluations: propsList.map((props) => parsed.evaluate(feature(props, "door"))),
          colors: (() => {
            try {
              return propsList.map((props) => parsed.evaluateColor(feature(props, "door")));
            } catch (e) {
              return { throws: e.name, message: e.message };
            }
          })(),
          shaderFunction: parsed.getShaderFunction("vec4 getColor(float x)", SHADER_MAP, state, "vec4") ?? null,
          translucent: state.translucent,
        };
      });
    }

    s.add("shader", [
      {
        conditions: [
          ["${Height} > 10", "color('red', 0.5)"],
          ["${Height} > 0", "vec4(1.0, 1.0, 0.0, 1.0)"],
          ["true", "vec4(0.0, 0.0, 1.0, 1.0)"],
        ],
      },
    ], (conditionsJson) => {
      const parsed = new ConditionsExpression(conditionsJson);
      const state = { translucent: false };
      return {
        shaderFunction: parsed.getShaderFunction("vec4 getColor(float x)", SHADER_MAP, state, "vec4"),
        translucent: state.translucent,
      };
    });

    for (const conditionsJson of [
      { conditions: [["${a} +", "'x'"]] },
      { conditions: [["true"]] },
      { conditions: [["unknownIdentifier", "'x'"]] },
    ]) {
      s.add("errors", [conditionsJson], (json) => {
        return { result: new ConditionsExpression(json).evaluate(feature(features[0], "door")) };
      });
    }
  }

  // ---- Cesium3DTileStyle ----
  {
    const s = add("Cesium3DTileStyle");
    const styles = [
      {
        show: "${Height} > 0",
        color: {
          conditions: [
            ["${Height} >= 10", "color('purple', 0.5)"],
            ["${Height} >= 0", "color('red')"],
            ["true", "color('blue')"],
          ],
        },
        pointSize: "${Height} / 10 + 1",
        meta: { description: "'Building id ${id} has height ${Height}.'", label: "'${name}'" },
        defines: { Height: "${h}" },
      },
      {
        show: true,
        color: "color('white')",
        pointSize: 2,
        pointOutlineColor: "color('black')",
        pointOutlineWidth: "2",
        labelColor: "color('cyan')",
        labelOutlineColor: "color('navy')",
        labelOutlineWidth: "3",
        labelStyle: "2",
        font: "'14px sans-serif'",
        labelText: "'${name}'",
        backgroundColor: "color('rgba(0,0,0,0.5)')",
        backgroundPadding: "vec2(4, 5)",
        backgroundEnabled: true,
        scaleByDistance: "vec4(1, 2, 3, 4)",
        translucencyByDistance: "vec4(5, 6, 7, 8)",
        distanceDisplayCondition: "vec2(10, 20)",
        heightOffset: "3.5",
        anchorLineEnabled: false,
        anchorLineColor: "color('yellow')",
        image: "'icon.png'",
        disableDepthTestDistance: "Number.POSITIVE_INFINITY",
        horizontalOrigin: "0",
        verticalOrigin: "1",
        labelHorizontalOrigin: "2",
        labelVerticalOrigin: "3",
      },
      {},
      {
        color: { conditions: [] },
        show: { conditions: [["true", "false"]] },
        pointSize: { conditions: [["${Area} > 5", "3"], ["true", "1"]] },
      },
    ];
    for (const styleJson of styles) {
      s.add("style", [styleJson, features], (json, propsList) => {
        const style = new Cesium3DTileStyle(json);
        const colorState = { translucent: false };
        const showState = { translucent: false };
        const pointSizeState = { translucent: false };
        const colorShader = style.getColorShaderFunction("vec4 getColor(float x)", SHADER_MAP, colorState) ?? null;
        const showShader = style.getShowShaderFunction("bool getShow(float x)", SHADER_MAP, showState) ?? null;
        const pointSizeShader = style.getPointSizeShaderFunction("float getPointSize(float x)", SHADER_MAP, pointSizeState) ?? null;
        return {
          style: JSON.parse(JSON.stringify(style.style)),
          variables: style.getVariables(),
          colorShader,
          showShader,
          pointSizeShader,
          translucent: [colorState.translucent, showState.translucent, pointSizeState.translucent],
          evaluations: {
            show: propsList.map((props) => style.show?.evaluate(feature(props, "door")) ?? null),
            color: propsList.map((props) => style.color?.evaluateColor(feature(props, "door")) ?? null),
            pointSize: propsList.map((props) => style.pointSize?.evaluate(feature(props, "door")) ?? null),
            meta: Object.fromEntries(
              Object.entries(style.meta).map(([key, expression]) => [
                key,
                propsList.map((props) => expression.evaluate(feature(props, "door"))),
              ]),
            ),
          },
          fields: Object.fromEntries(
            [
              "pointOutlineColor",
              "pointOutlineWidth",
              "labelColor",
              "labelOutlineColor",
              "labelOutlineWidth",
              "labelStyle",
              "font",
              "labelText",
              "backgroundColor",
              "backgroundPadding",
              "backgroundEnabled",
              "scaleByDistance",
              "translucencyByDistance",
              "distanceDisplayCondition",
              "heightOffset",
              "anchorLineEnabled",
              "anchorLineColor",
              "image",
              "disableDepthTestDistance",
              "horizontalOrigin",
              "verticalOrigin",
              "labelHorizontalOrigin",
              "labelVerticalOrigin",
            ].map((key) => [key, style[key]?.evaluate(feature(features[0], "door")) ?? null]),
          ),
        };
      });
    }

    // Setters normalize JSON and invalidate shader caches.
    s.add("setters", [features], (propsList) => {
      const style = new Cesium3DTileStyle();
      style.show = true;
      style.color = "color('red')";
      style.pointSize = 4;
      const before = {
        style: JSON.parse(JSON.stringify(style.style)),
        show: style.show.evaluate(feature(propsList[0], "door")),
        color: style.color.evaluateColor(feature(propsList[0], "door")),
        pointSize: style.pointSize.evaluate(feature(propsList[0], "door")),
      };
      const state = { translucent: false };
      const shader = style.getColorShaderFunction("vec4 f(float x)", SHADER_MAP, state);
      style.color = { conditions: [["true", "color('blue', 0.25)"]] };
      const after = {
        style: JSON.parse(JSON.stringify(style.style)),
        shader: style.getColorShaderFunction("vec4 f(float x)", SHADER_MAP, { translucent: false }),
        color: style.color.evaluateColor(feature(propsList[0], "door")),
      };
      return { before, shader, after };
    });

    for (const json of [
      { show: { unknown: 1 } },
      { color: [1, 2] },
      { meta: { description: 5 } },
    ]) {
      s.add("errors", [json], (styleJson) => {
        new Cesium3DTileStyle(styleJson);
        return { ok: true };
      });
    }
  }

  return out;
}
