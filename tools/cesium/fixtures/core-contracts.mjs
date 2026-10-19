import * as C from '@cesium/engine';
import {suite,makeRng} from './lib.mjs';
export default function coreContracts(){
 const s=suite('Packable'),rng=makeRng(991);
 for(const name of ['Cartesian2','Cartesian3','Cartesian4','Matrix2','Matrix3','Matrix4','Quaternion','Color','BoundingRectangle','BoundingSphere','Rectangle','Ellipsoid','NearFarScalar','DistanceDisplayCondition','VertexFormat'])
  for(const offset of [0,3]){
   const values=Array.from({length:C[name].packedLength},()=>name==='VertexFormat'?Math.round(rng()):rng()*5);
   s.add('pack',[name,values,offset],(name,values,offset)=>{const t=C[name],v=t.unpack(values),out=Array(t.packedLength+offset+2).fill(-999);t.pack(v,out,offset);const clone=t.unpack(out,offset),copy=Array(t.packedLength);t.pack(clone,copy);return [t.packedLength,out,copy];});
  }
 const g=suite('GeometryFactory');
 for(const shape of ['Box','Plane','Sphere','Cylinder'])g.add('create',[shape],shape=>{
  const d=shape==='Box'?C.BoxGeometry.fromDimensions({dimensions:new C.Cartesian3(2,3,4)}):shape==='Plane'?new C.PlaneGeometry():shape==='Sphere'?new C.SphereGeometry({radius:2,stackPartitions:4,slicePartitions:5}):new C.CylinderGeometry({length:3,topRadius:1,bottomRadius:2,slices:8});
  const v=C[shape+'Geometry'].createGeometry(d);return [v.primitiveType,v.indices.length,v.attributes.position.values.length,v.boundingSphere.radius];
 });
 return [s,g];
}
