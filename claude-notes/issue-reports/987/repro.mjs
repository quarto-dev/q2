const re=/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/;
for (const mb of [0.5,1,1.5,2,2.5,3,3.5,4,5,10]) {
  const s=Buffer.alloc(Math.round(mb*1048576),7).toString('base64');
  try { console.log(mb,'MB chars',s.length,re.test(s)); } catch(e){ console.log(mb,'MB chars',s.length,e.name,e.message); }
}
