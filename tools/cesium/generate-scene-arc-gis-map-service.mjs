// Reference defaults and credit from pinned @cesium/engine 23.0.0.
import { writeFileSync } from 'node:fs';
import service from '@cesium/engine/Source/Scene/ArcGisMapService.js';
const token = service.defaultAccessToken;
const credit = service.getDefaultTokenCredit(token);
const result = {
  token,
  imagery: service.defaultWorldImageryServer.url,
  hillshade: service.defaultWorldHillshadeServer.url,
  ocean: service.defaultWorldOceanServer.url,
  credit: { html: credit.html, show: credit.showOnScreen },
  otherTokenHasCredit: service.getDefaultTokenCredit('my-token') !== undefined,
  sameCredit: service.getDefaultTokenCredit(token) === credit,
};
writeFileSync(new URL('../../tests/fixtures/scene/ArcGisMapService.json', import.meta.url), JSON.stringify({ version: '23.0.0', cases: [result] }, null, 2) + '\n');
