(() => {
 'use strict';
 const KEY = '__overlayMusicBridge';
 window[KEY]?.dispose?.();
 const root = document.getElementById('root');
 let store = root?._reactRootContainer?._internalRoot?.current?.child?.child?.memoizedProps?.store;
 const child = root?.firstElementChild;
 const fiberKey = Object.keys(child || {}).find(k => /^__react(Fiber|InternalInstance)/.test(k));
 for (let f = child?.[fiberKey], i = 0; f && i < 80; f = f.return, i++) {
   if (f.memoizedProps?.store) { store = f.memoizedProps.store; break; }
 }
 if (!store?.getState || !store?.subscribe || !store?.getState().playing) throw Error('未找到兼容的播放器状态源（需要网易云 3.x）');
 const button = id => {
   const els = [...document.querySelectorAll(`[data-testid="${id}"]`)];
   return els.find(el => el.getClientRects().length && !el.closest('[aria-hidden="true"]')) || els[0];
 };
 let dislikeSelection = null, modeRequest = null, modeError = '', modeTimer;
 const dislikeModal = () => document.querySelector('[data-testid="tid_blacklist_container"]')?.closest('.cmd-modal,.ant-modal,[role="dialog"]') || document.querySelector('.modal-unlikeFm');
 const closeDislike = () => { if(dislikeSelection?.modal?.isConnected) dislikeSelection.modal.querySelector('button.close,.cmd-modal-close,.ant-modal-close,[aria-label="Close"],[aria-label="关闭"]')?.click();dislikeSelection=null; };
 const snapshot = () => {
   const p = store.getState().playing;
   const like = button('tid_redstar_btn');
   const label = like?.querySelector('[title]')?.title || '';
   return { connected:true, id:String(p.resourceTrackId || ''), title:p.resourceName || '尚未播放歌曲',
     artist:(p.resourceArtists || []).map(a => a.name).join(' / '), playing:p.playingState === 2,
     volume:Math.round((p.playingVolume || 0) * 100), mode:p.playingMode,
     liked:/取消喜欢|已喜欢/.test(label), canLike:!!like, canDislike:!!p.resourceTrackId,
     fm:p.playingMode === 'playFm', canAI:!!p.isCanHeartBeat,
     modePending:modeRequest?.value || '', operationError:modeError };
 };
 let last = '', timer, lease;
 const publish = () => {
   timer = undefined;
   if(typeof window.__overlayMusicNotify !== 'function'){dispose();return;}
   const value = JSON.stringify(snapshot());
   if (value !== last) { last = value; window.__overlayMusicNotify(value); }
 };
 const schedule = () => { if (!timer) timer = setTimeout(publish, 60); };
 const unsubscribe = store.subscribe(schedule);
 // Like state is not in the playing slice; observe only the two existing buttons.
 const observer = new MutationObserver(schedule);
 for (const el of document.querySelectorAll('[data-testid="tid_redstar_btn"]')) observer.observe(el, {subtree:true,attributes:true,childList:true});
 const touch = () => { clearTimeout(lease); lease = setTimeout(dispose, 45000); };
 function dispose() { unsubscribe(); observer.disconnect(); clearTimeout(timer); clearTimeout(lease); clearTimeout(modeTimer);modeRequest=null;closeDislike(); if(window[KEY]?.snapshot === snapshot) delete window[KEY]; }
 const dispatch = (type,payload) => store.dispatch({type,payload});
 const click = id => { const el=button(id); if(!el || el.disabled || el.getAttribute('aria-disabled') === 'true') throw Error('客户端此操作当前不可用'); el.click(); };
 function switchMode(value) {
   if(modeRequest)throw Error('正在等待网易云切换模式，请稍候');
   if(!['playOrder','playCycle','playOneCycle','playRandom','playAi','playFm'].includes(value))throw Error('无效播放模式');
   modeError='';
   const state=store.getState(),p=state.playing;
   const sameFm=value==='playFm'&&p.playingMode===value&&state.fmPlaying?.currentScene==='fm'&&!state.fmPlaying?.currentMode;
   if((value==='playAi'&&p.playingMode===value)||sameFm){if(p.playingState!==2)dispatch('playing/resume',{triggerScene:'sysTray'});return;}
   const request={value};modeRequest=request;
   const finish=error=>{
     if(modeRequest!==request)return;
     modeRequest=null;clearTimeout(modeTimer);
     modeError=error || (store.getState().playing.playingMode!==value?'网易云未确认模式切换，请检查登录、网络或可播放歌曲':'');schedule();
   };
   modeTimer=setTimeout(()=>finish('模式切换尚未确认，请检查网易云客户端；不会自动重试'),8000);
   try{
     // Use native entry points: playFm alone is only a marker, not playback.
     const result=value==='playFm'?dispatch('fmPlaying/fmPlay',{scene:'fm'}):value==='playAi'?dispatch('playing/aiPlay',{isFromPlayAll:true,isFromIconClick:true,triggerScene:'sysTray'}):dispatch('playing/switchPlayingMode',{playingMode:value,triggerScene:'sysTray'});
     if(result?.then)Promise.resolve(result).then(()=>finish(),()=>finish('网易云模式切换失败，请在客户端检查后重试'));
     else if(store.getState().playing.playingMode===value)finish();
   }catch(error){finish(error.message);throw error;}
 }
 async function command(cmd, value) {
   touch();modeError='';
   switch(cmd) {
     case 'toggle': dispatch(snapshot().playing?'playing/pause':'playing/resume',{triggerScene:'desktopLyric'}); break;
     case 'previous': click('tid_playbar_prev_btn'); break;
     case 'next': click('tid_playbar_next_btn'); break;
     case 'like': {
       const before=snapshot();click('tid_redstar_btn');
       for(let i=0;i<20;i++){await new Promise(r=>setTimeout(r,100));const now=snapshot();if(now.id!==before.id)throw Error('歌曲已改变，请检查网易云的喜欢状态');if(now.liked!==before.liked)break;if(i===19)throw Error('网易云尚未确认喜欢状态，请检查网络或登录状态');}
       break;
     }
     case 'dislike': {
       if(dislikeModal())throw Error('网易云已有反馈框，请先在客户端关闭');
       const p=store.getState().playing,resource=p.curTrack||p.curPlaying?.track;
       if(p.resourceType&&!['track','cloudTrack','localTrack','downloadTrack'].includes(p.resourceType))throw Error('当前资源不是歌曲，网易云可能不提供歌曲反馈；请在客户端查看');
       if(!resource?.id)throw Error('当前歌曲信息尚未就绪，请稍后重试');
       const id=snapshot().id;
       dispatch('playing/openDislikeModal',{resource,resourceType:'track',scene:p.curPlaying?.scene||'playingList'});
       for(let i=0;i<20;i++) {
         await new Promise(r=>setTimeout(r,100));
         const modal=dislikeModal();
         if(modal && snapshot().id!==id){dislikeSelection={id,modal};closeDislike();throw Error('歌曲已改变，请重新打开不喜欢界面');}
         const items=[...(modal?.querySelectorAll('.content-item')||[])].filter(el=>el.textContent.trim()&&!el.classList.contains('disabled')&&el.getAttribute('aria-disabled')!=='true').slice(0,20);
         if(items.length) {const labels=items.map(el=>el.textContent.trim());dislikeSelection={id,modal,items,labels};return {...snapshot(),dislikeChoices:labels};}
         // Compatibility for older clients without the common blacklist entry.
         if(i===9&&!modal&&snapshot().fm)dispatch('fmPlaying/openDislikeModal',{});
       }
       throw Error('未识别到网易云反馈选项，请在客户端查看是否已打开；不会自动提交或跳过歌曲');
     }
     case 'dislikeReason': {
       const pending=dislikeSelection;
       if(!pending || pending.id!==snapshot().id || !pending.modal?.isConnected || !Number.isInteger(value) || !pending.items[value]?.isConnected || !pending.modal.contains(pending.items[value])){closeDislike();throw Error('歌曲已改变或原因框已关闭，请重新选择');}
       const item=pending.items[value];
       const label=item.textContent.trim();
       if(label!==pending.labels[value]){closeDislike();throw Error('反馈选项已变化，请重新打开后选择');}
       if(item.classList.contains('disabled')||item.getAttribute('aria-disabled')==='true')throw Error('该反馈选项当前不可用，请重新打开');
       // Release the selection before clicking: native submission can change
       // tracks immediately. Never close a native follow-up confirmation.
       dislikeSelection=null;item.click();
       await new Promise(resolve=>setTimeout(resolve,180));
       return {...snapshot(),controlMessage:'已选择“'+label+'”；若网易云出现后续确认，请在客户端完成'};
     }
     case 'dislikeCancel': closeDislike();break;
     case 'mute': dispatch('playing/switchMute'); break;
     case 'volume': if(!Number.isInteger(value)||value<0||value>100)throw Error('音量必须为 0–100');dispatch('playing/setVolume',{volume:value/100});break;
     case 'mode': switchMode(value);break;
     default: throw Error('未知操作');
   }
   schedule();
   // Native actions update Redux asynchronously; don't return the pre-action state.
   await new Promise(resolve => setTimeout(resolve, 180));
   return {...snapshot(),...(cmd==='mode'&&modeRequest?{controlMessage:'已请求切换模式，正在等待网易云确认'}:{})};
 }
 async function finishMedia(cmd,before,sendError) {
   if(!['toggle','previous','next'].includes(cmd) || !before?.connected || typeof before.playing!=='boolean')throw Error('无效媒体确认请求');
   touch();
   const desired=!before.playing;
   const responded=now=>now.id!==before.id || (cmd==='toggle' && now.playing===desired);
   const reply=(result,message)=>({...snapshot(),controlResult:result,controlMessage:message});
   // A bounded confirmation window. No continuous polling while idle.
   if(!sendError)for(let i=0;i<8;i++){
     await new Promise(resolve=>setTimeout(resolve,150));
     if(responded(snapshot()))return reply('windows','Windows 媒体控制已响应');
   }
   if(responded(snapshot()))return reply('windows','播放器状态已变化，未重复执行');
   if(cmd==='toggle'){
     // Set a target state rather than toggling again after a delayed media key.
     dispatch(desired?'playing/resume':'playing/pause',{triggerScene:'desktopLyric'});
     schedule();
   }else{await command(cmd);}
   for(let i=0;i<8;i++){
     await new Promise(resolve=>setTimeout(resolve,100));
     if(responded(snapshot()))return reply('ncm-fallback','Windows 未响应，已由网易云控制兜底');
   }
   return reply('unconfirmed','已执行网易云兜底，但尚未确认状态变化；不会继续重试');
 }
 window[KEY] = {snapshot,command,finishMedia,touch,dispose}; touch(); publish(); return snapshot();
})()
