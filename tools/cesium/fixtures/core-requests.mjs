import {Request,RequestScheduler as S} from '@cesium/engine';
import {enc} from './lib.mjs';
export default async function coreRequests(){
 const s={module:'RequestScheduler',cases:[]};
 for(const mode of ['success','failure','cancel']) for(const priorities of [[5,1,3,2],[1,2,3,4],[4,3,2,1]]){
  S.clearForSpecs();S.maximumRequests=2;S.maximumRequestsPerServer=1;S.priorityHeapLength=3;
  const started=[],cancelled=[],completed=[],resolvers=new Map(),requests=[],promises=[];
  const remove=S.requestCompletedEvent.addEventListener(e=>completed.push(e?'error':'ok'));
  for(let i=0;i<priorities.length;i++){
   const request=new Request({url:'https://request.test/'+i,priority:priorities[i],throttle:true,requestFunction:()=>{started.push(i);return new Promise((resolve,reject)=>resolvers.set(i,{resolve,reject}));},cancelFunction:()=>cancelled.push(i)});
   requests.push(request);const promise=S.request(request);promises.push(promise);promise?.catch(()=>{});
  }
  const snapshot=()=>[requests.map(r=>r.state),started.slice(),cancelled.slice(),completed.slice(),[S.statistics.numberOfAttemptedRequests,S.statistics.numberOfActiveRequests,S.statistics.numberOfCancelledRequests,S.statistics.numberOfCancelledActiveRequests,S.statistics.numberOfFailedRequests,S.statistics.numberOfActiveRequestsEver]];
  const steps=[snapshot()];S.update();steps.push(snapshot());
  for(const [i,r] of resolvers){if(mode==='cancel')requests[i].cancel();else if(mode==='failure')r.reject('bad');else r.resolve(i);}
  await Promise.resolve();await Promise.resolve();await Promise.resolve();S.update();steps.push(snapshot());
  for(const [i,r] of resolvers)r.resolve(i);await Promise.resolve();await Promise.resolve();await Promise.resolve();S.update();steps.push(snapshot());
  s.cases.push({f:'lifecycle',a:[mode,priorities],r:enc(steps)});remove();
 }
 S.clearForSpecs();S.maximumRequests=50;S.maximumRequestsPerServer=18;S.priorityHeapLength=20;return [s];
}
