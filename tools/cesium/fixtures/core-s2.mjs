import { Ellipsoid, S2Cell } from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

export default function coreS2() {
  const s=suite("S2Cell"),g=generators(makeRng(2100));
  const cell=(id,ellipsoid)=>{
    const c=new S2Cell(BigInt(id)),level=c._level;
    return [c._cellId.toString(),level,S2Cell.getTokenFromId(c._cellId),level?c.getParent()._cellId.toString():null,
      level<30?Array.from({length:4},(_,i)=>c.getChild(i)._cellId.toString()):null,
      level?Array.from({length:level+1},(_,i)=>c.getParentAtLevel(i)._cellId.toString()):null,
      c.getCenter(Ellipsoid[ellipsoid]),Array.from({length:4},(_,i)=>c.getVertex(i,Ellipsoid[ellipsoid]))];
  };
  for(let face=0;face<6;face++){
    const root=(BigInt(face)<<61n)|(1n<<60n);
    for(const name of ["WGS84","MOON","UNIT_SPHERE"])s.add("cell",[root.toString(),name],cell);
    for(const level of [0,1,2,7,15,29,30]){
      const limit=1n<<BigInt(level*2);
      const positions=[0n,limit-1n];
      for(let j=0;j<6;j++)positions.push((BigInt(Math.floor(g.rng()*2**30))<<30n|BigInt(Math.floor(g.rng()*2**30)))%limit);
      for(const position of positions){
        // Level-zero face construction has an extra zero bit in Cesium.
        s.add("fromFace",[face,position.toString(),level],(f,p,l)=>{try{return S2Cell.fromFacePositionLevel(f,BigInt(p),l)._cellId.toString();}catch(e){return null;}});
        const id=(BigInt(face)<<61n)|((position*2n+1n)<<BigInt(60-2*level));
        s.add("cell",[id.toString(),g.pick(["WGS84","MOON","UNIT_SPHERE"])],cell);
      }
    }
  }
  for(const token of ["","0","0000000000000000","1","3","5","7","9","b","B","2c","0000000000000001","89c259c4","a1b2c3d4e5f67891","ffffffffffffffff","c","2","xyz"," 1","10000000000000000"]){
    s.add("token",[token],token=>[S2Cell.isValidToken(token),/^[0-9a-fA-F]{1,16}$/.test(token)?S2Cell.getIdFromToken(token).toString():null,S2Cell.isValidToken(token)?S2Cell.fromToken(token)._cellId.toString():null]);
  }
  for(const id of [0n,1n,2n,3n,4n,8n,12n,1n<<60n,5n<<61n,6n<<61n,(1n<<64n)-1n]){
    s.add("id",[id.toString()],id=>{const n=BigInt(id);return[S2Cell.isValidId(n),S2Cell.getTokenFromId(n),S2Cell.isValidId(n)?S2Cell.getLevel(n):null];});
  }
  return [s];
}
