package dev.wunder.android;

/**
 * Page scripts injected by {@link WebViewBridge}.
 *
 * <p>Every script returns one JSON string envelope such as
 * {@code {"status":"selected",...}}. The bridge treats a missing, empty or
 * unparsable envelope as an injection failure, so a script that never ran can
 * no longer be reported as a successful action.
 */
final class BrowserScripts {
    static final int MAX_SNAPSHOT_NODES = 220;

    /**
     * Resolve visible text to the smallest visible interactive element whose
     * accessible name contains it. Exact names win over prefix/substring hits,
     * and the smallest box wins among equals so a nested button is preferred
     * over its wrapping card.
     */
    private static final String FIND_BY_TEXT =
            "function __wunderFindByText(raw){"
            + "var t=String(raw==null?'':raw).replace(/\\s+/g,' ').trim();"
            + "if(!t)return null;"
            + "var sel='a,button,input,textarea,select,summary,label,"
            + "[role=\"button\"],[role=\"link\"],[role=\"checkbox\"],[role=\"radio\"],"
            + "[role=\"tab\"],[role=\"menuitem\"],[contenteditable=\"true\"],[onclick]';"
            + "var all=document.querySelectorAll(sel);"
            + "var best=null,bestArea=Infinity,bestExact=false;"
            + "for(var i=0;i<all.length;i++){var el=all[i];"
            + "var name=el.getAttribute('aria-label')||el.getAttribute('title')||el.getAttribute('placeholder')||el.getAttribute('alt')||el.innerText||'';"
            + "name=String(name).replace(/\\s+/g,' ').trim();"
            + "if(!name)continue;"
            + "var exact=name===t;"
            + "if(!exact&&name.indexOf(t)<0)continue;"
            + "var r;try{r=el.getBoundingClientRect();}catch(e){continue;}"
            + "if(r.width<=2||r.height<=2)continue;"
            + "var s;try{s=getComputedStyle(el);}catch(e){s=null;}"
            + "if(s&&(s.visibility==='hidden'||s.display==='none'||parseFloat(s.opacity||'1')<=0.05))continue;"
            + "var area=r.width*r.height;"
            + "if((exact&&!bestExact)||(exact===bestExact&&area<bestArea)){best=el;bestArea=area;bestExact=exact;}}"
            + "return best;}";

    private BrowserScripts() {}

    static String click(String target, boolean doubleClick) {
        return "(function(){" + FIND_BY_TEXT
                + "var el=" + target + ";"
                + "if(!el)return JSON.stringify({status:'not_found'});"
                + "try{el.scrollIntoView({block:'center',inline:'center'});}catch(e){}"
                + "var r=el.getBoundingClientRect();"
                + "var x=r.left+r.width/2,y=r.top+r.height/2;"
                + "['pointerover','mouseover','pointerdown','mousedown','pointerup','mouseup']"
                + ".forEach(function(t){try{el.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x,clientY:y}));}catch(e){}});"
                + "try{el.click();}catch(e){return JSON.stringify({status:'error',message:String(e)});}"
                + (doubleClick
                        ? "try{el.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,cancelable:true,clientX:x,clientY:y}));}catch(e){}"
                        : "")
                + "return JSON.stringify({status:'clicked',tag:el.tagName||'',"
                + "label:String(el.getAttribute('aria-label')||el.innerText||el.value||'').replace(/\\s+/g,' ').trim().slice(0,60)});"
                + "})()";
    }

    static String type(String target, String text, boolean submit) {
        String value = quote(text);
        return "(function(){" + FIND_BY_TEXT
                + "var el=" + target + ";"
                + "if(!el)return JSON.stringify({status:'not_found'});"
                + "var v=" + value + ";"
                + "try{el.focus();}catch(e){}"
                + "if(el.isContentEditable){el.textContent=v;}"
                + "else if(el.tagName==='INPUT'||el.tagName==='TEXTAREA'){"
                + "var proto=el.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype;"
                + "var desc=Object.getOwnPropertyDescriptor(proto,'value');"
                + "if(desc&&desc.set){desc.set.call(el,v);}else{el.value=v;}"
                + "}else{return JSON.stringify({status:'not_editable',tag:el.tagName||''});}"
                + "try{el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));}catch(e){}"
                + (submit
                        ? "try{var f=el.form;if(f&&f.requestSubmit){f.requestSubmit();}else{"
                        + "['keydown','keypress','keyup'].forEach(function(t){el.dispatchEvent(new KeyboardEvent(t,{key:'Enter',code:'Enter',keyCode:13,which:13,bubbles:true}));});}}catch(e){}"
                        : "")
                + "return JSON.stringify({status:'typed',value:String(el.value!=null?el.value:(el.textContent||'')).slice(0,200)});"
                + "})()";
    }

    static String hover(String target) {
        return "(function(){" + FIND_BY_TEXT
                + "var el=" + target + ";"
                + "if(!el)return JSON.stringify({status:'not_found'});"
                + "var r=el.getBoundingClientRect();"
                + "['pointerover','mouseover','mousemove'].forEach(function(t){try{el.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:r.left+r.width/2,clientY:r.top+r.height/2}));}catch(e){}});"
                + "return JSON.stringify({status:'hovered'});"
                + "})()";
    }

    static String focus(String target) {
        return "(function(){" + FIND_BY_TEXT
                + "var el=" + target + ";"
                + "if(!el)return JSON.stringify({status:'not_found'});"
                + "try{el.scrollIntoView({block:'center',inline:'center'});}catch(e){}"
                + "try{el.focus();}catch(e){}"
                + "var editable=!!(el.isContentEditable||el.tagName==='INPUT'||el.tagName==='TEXTAREA'||el.tagName==='SELECT');"
                + "return JSON.stringify({status:'focused',tag:el.tagName||'',editable:editable});"
                + "})()";
    }

    static String select(String target, String valueExpression) {
        return "(function(){" + FIND_BY_TEXT
                + "var el=" + target + ";"
                + "if(!el)return JSON.stringify({status:'not_found'});"
                + "if(!el.options||typeof el.options.length!=='number')return JSON.stringify({status:'not_select',tag:el.tagName||''});"
                + "var values=" + valueExpression + ";"
                + "if(!Array.isArray(values))values=[values];"
                + "var norm=function(v){return String(v==null?'':v).replace(/\\s+/g,' ').trim();};"
                + "var want=[];for(var i=0;i<values.length;i++){var w=norm(values[i]);if(w)want.push(w);}"
                + "if(!want.length)return JSON.stringify({status:'empty'});"
                + "var opts=[];for(var i=0;i<el.options.length;i++)opts.push(el.options[i]);"
                + "var matched=[];"
                + "function labelOf(o){return norm(o.label!=null?o.label:o.text);}"
                + "function collect(compare){var out=[];for(var i=0;i<opts.length;i++){var o=opts[i];var v=norm(o.value),l=labelOf(o);"
                + "for(var j=0;j<want.length;j++){if(compare(v,l,want[j])){out.push(o);break;}}}return out;}"
                + "matched=collect(function(v,l,w){return v===w||l===w;});"
                + "if(!matched.length)matched=collect(function(v,l,w){return l.indexOf(w)>=0;});"
                + "if(!matched.length){var avail=[];for(var i=0;i<opts.length&&i<20;i++){var o=opts[i];"
                + "var v=norm(o.value),l=labelOf(o);avail.push(l&&l!==v?v+'('+l+')':v);}"
                + "return JSON.stringify({status:'no_match',options:avail,count:opts.length});}"
                + "if(!el.multiple&&matched.length>1)matched=[matched[0]];"
                + "for(var i=0;i<opts.length;i++)opts[i].selected=false;"
                + "for(var i=0;i<matched.length;i++)matched[i].selected=true;"
                + "try{el.dispatchEvent(new Event('input',{bubbles:true}));el.dispatchEvent(new Event('change',{bubbles:true}));}catch(e){}"
                + "var selected=[];for(var i=0;i<el.options.length;i++){var o=el.options[i];"
                + "if(o.selected){var l=labelOf(o);selected.push(l||norm(o.value));}}"
                + "return JSON.stringify({status:'selected',selected:selected,value:String(el.value==null?'':el.value)});"
                + "})()";
    }

    static String dragElements(String from, String to, int steps) {
        return "(function(){var a=" + from + ",b=" + to + ";if(!a||!b)return JSON.stringify({status:'not_found'});"
                + "var r1=a.getBoundingClientRect(),r2=b.getBoundingClientRect();"
                + "var x1=r1.left+r1.width/2,y1=r1.top+r1.height/2,x2=r2.left+r2.width/2,y2=r2.top+r2.height/2;"
                + "['pointerdown','mousedown'].forEach(function(t){try{a.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x1,clientY:y1}));}catch(e){}});"
                + "for(var i=1;i<=" + steps + ";i++){var f=i/" + steps + ";var mx=x1+(x2-x1)*f,my=y1+(y2-y1)*f;"
                + "var target=document.elementFromPoint(mx,my)||a;"
                + "['pointermove','mousemove'].forEach(function(t){try{target.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:mx,clientY:my}));}catch(e){}});}"
                + "['pointerup','mouseup'].forEach(function(t){try{b.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x2,clientY:y2}));}catch(e){}});"
                + "return JSON.stringify({status:'dragged'});})()";
    }

    static String dragCoordinates(int fromX, int fromY, int toX, int toY, int steps) {
        return "(function(){var x1=" + fromX + ",y1=" + fromY + ",x2=" + toX + ",y2=" + toY + ";"
                + "var a=document.elementFromPoint(x1,y1);if(!a)return JSON.stringify({status:'not_found'});"
                + "['pointerdown','mousedown'].forEach(function(t){try{a.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x1,clientY:y1}));}catch(e){}});"
                + "for(var i=1;i<=" + steps + ";i++){var f=i/" + steps + ";var mx=x1+(x2-x1)*f,my=y1+(y2-y1)*f;"
                + "var target=document.elementFromPoint(mx,my)||a;"
                + "['pointermove','mousemove'].forEach(function(t){try{target.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:mx,clientY:my}));}catch(e){}});}"
                + "var b=document.elementFromPoint(x2,y2)||a;"
                + "['pointerup','mouseup'].forEach(function(t){try{b.dispatchEvent(new MouseEvent(t,{bubbles:true,cancelable:true,clientX:x2,clientY:y2}));}catch(e){}});"
                + "return JSON.stringify({status:'dragged'});})()";
    }

    /**
     * A real wheel event plus programmatic scrolling. The wheel lands at the
     * requested point so canvas/OrbitControls pages receive it; if the page
     * calls preventDefault the wheel owns the gesture and no extra scroll is
     * added. Otherwise whatever the handler already scrolled synchronously is
     * subtracted and the rest is applied to the nearest scrollable ancestor.
     */
    static String scroll(int deltaX, int deltaY, int pointX, int pointY) {
        return "(function(){"
                + "var dx=" + deltaX + ",dy=" + deltaY + ",px=" + pointX + ",py=" + pointY + ";"
                + "var pt=null;if(px>=0&&py>=0){try{pt=document.elementFromPoint(px,py);}catch(e){}}"
                + "var cx=px>=0?px:(window.innerWidth||0)/2,cy=py>=0?py:(window.innerHeight||0)/2;"
                + "var wheelAt=pt||document.elementFromPoint(cx,cy)||document.body||document.documentElement;"
                + "var handled=false;"
                + "try{handled=!wheelAt.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,deltaX:dx,deltaY:dy,clientX:cx,clientY:cy}));}catch(e){}"
                + "function scrollable(el){if(!el||el.nodeType!==1)return false;try{var cs=getComputedStyle(el);"
                + "var okY=(dy!==0)&&(cs.overflowY==='auto'||cs.overflowY==='scroll')&&el.scrollHeight>el.clientHeight+1;"
                + "var okX=(dx!==0)&&(cs.overflowX==='auto'||cs.overflowX==='scroll')&&el.scrollWidth>el.clientWidth+1;"
                + "return okY||okX;}catch(e){return false;}}"
                + "var target=null,node=pt;"
                + "while(node&&node.nodeType===1&&node!==document.body&&node!==document.documentElement){"
                + "if(scrollable(node)){target=node;break;}node=node.parentElement;}"
                + "var beforeX=window.scrollX||0,beforeY=window.scrollY||0;"
                + "var beforeL=target?target.scrollLeft:0,beforeT=target?target.scrollTop:0;"
                + "var movedX=(window.scrollX||0)-beforeX,movedY=(window.scrollY||0)-beforeY;"
                + "if(target){movedX+=target.scrollLeft-beforeL;movedY+=target.scrollTop-beforeT;}"
                + "var restX=handled?0:dx-movedX,restY=handled?0:dy-movedY;"
                + "if(restX!==0||restY!==0){try{if(target){target.scrollBy({left:restX,top:restY,behavior:'auto'});}else{window.scrollBy(restX,restY);}}catch(e){}}"
                + "var afterX=window.scrollX||0,afterY=window.scrollY||0;"
                + "var afterL=target?target.scrollLeft:0,afterT=target?target.scrollTop:0;"
                + "var moved=((afterX-beforeX)+(afterL-beforeL))!==0||((afterY-beforeY)+(afterT-beforeT))!==0;"
                + "return JSON.stringify({status:'scrolled',moved:moved,handled:handled,x:afterX,y:afterY,"
                + "container:target?String(target.tagName||'').toLowerCase():'document',"
                + "canvas:!!(pt&&String(pt.tagName||'').toUpperCase()==='CANVAS')});"
                + "})()";
    }

    /**
     * One polling tick for wait: every provided condition must hold. Returns
     * which conditions passed and which are still missing.
     */
    static String waitCheck(String selector, String state, String text, String url,
                            String loadState) {
        StringBuilder js = new StringBuilder("(function(){var failed=[],done=[];");
        if (selector != null) {
            String sel = quote(selector);
            String expected = state == null ? "" : state;
            String expr;
            if ("hidden".equals(expected)) {
                expr = "(function(){var el=document.querySelector(" + sel + ");if(!el)return true;"
                        + "var r=el.getBoundingClientRect();var s=getComputedStyle(el);"
                        + "return !(r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&parseFloat(s.opacity||'1')>0.05);})()";
            } else if ("detached".equals(expected)) {
                expr = "(document.querySelector(" + sel + ")==null)";
            } else if ("attached".equals(expected)) {
                expr = "(document.querySelector(" + sel + ")!=null)";
            } else {
                expr = "(function(){var el=document.querySelector(" + sel + ");if(!el)return false;"
                        + "var r=el.getBoundingClientRect();var s=getComputedStyle(el);"
                        + "return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&parseFloat(s.opacity||'1')>0.05;})()";
            }
            js.append("if(").append(expr).append(")done.push('selector');else failed.push('selector');");
        }
        if (text != null) {
            js.append("if(((document.body&&document.body.innerText)||'').indexOf(")
                    .append(quote(text))
                    .append(")>=0)done.push('text');else failed.push('text');");
        }
        if (url != null) {
            js.append("(function(){var p=").append(quote(url))
                    .append(",h=location.href,ok=false;")
                    .append("if(p===h||h.indexOf(p)>=0)ok=true;")
                    .append("else if(p.indexOf('*')>=0){var parts=p.split('*'),idx=0,glob=true;")
                    .append("for(var i=0;i<parts.length;i++){if(!parts[i])continue;")
                    .append("var at=h.indexOf(parts[i],idx);if(at<0){glob=false;break;}idx=at+parts[i].length;}ok=glob;}")
                    .append("if(ok)done.push('url');else failed.push('url');})();");
        }
        if (loadState != null) {
            String expr = "domcontentloaded".equals(loadState)
                    ? "(document.readyState==='interactive'||document.readyState==='complete')"
                    : "(document.readyState==='complete')";
            js.append("if(").append(expr).append(")done.push('load_state');else failed.push('load_state');");
        }
        js.append("return JSON.stringify({status:'check',ok:failed.length===0,failed:failed,done:done,"
                + "href:location.href,readyState:document.readyState});})()");
        return js.toString();
    }

    static String readText() {
        return "(function(){var b=document.body;"
                + "return JSON.stringify({status:'ok',text:b?(b.innerText||b.textContent||''):''});})()";
    }

    static String fileUpload(String target, String filesJson) {
        return "(function(){" + FIND_BY_TEXT
                + "var input=" + target + ";"
                + "if(!input)return JSON.stringify({status:'not_found'});"
                + "if(input.tagName==='LABEL'){input=input.control"
                + "||(input.htmlFor?document.getElementById(input.htmlFor):null)"
                + "||input.querySelector('input[type=file]');}"
                + "if(!input||input.tagName!=='INPUT'||String(input.type).toLowerCase()!=='file')"
                + "return JSON.stringify({status:'not_file_input',tag:input?String(input.tagName||''):''});"
                + "var data=" + filesJson + ";"
                + "var dt;try{dt=new DataTransfer();}catch(e){return JSON.stringify({status:'assign_failed',message:'DataTransfer 不可用'});}"
                + "for(var i=0;i<data.length;i++){var bin=atob(data[i].b64);var arr=new Uint8Array(bin.length);"
                + "for(var j=0;j<bin.length;j++)arr[j]=bin.charCodeAt(j);"
                + "try{dt.items.add(new File([arr],data[i].name,{type:data[i].type||'application/octet-stream',lastModified:Date.now()}));}"
                + "catch(e){return JSON.stringify({status:'assign_failed',message:String(e)});}}"
                + "try{input.files=dt.files;}catch(e){return JSON.stringify({status:'assign_failed',message:String(e)});}"
                + "try{input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));}catch(e){}"
                + "var names=[];try{for(var i=0;i<input.files.length;i++)names.push(input.files[i].name);}catch(e){}"
                + "return JSON.stringify({status:'uploaded',count:names.length,names:names});"
                + "})()";
    }

    /**
     * Builds an accessibility-like tree and assigns stable `data-wunder-ref`
     * attributes so the model can target elements without CSS knowledge.
     */
    static String snapshot() {
        return "(function(){"
                + "var old=document.querySelectorAll('[data-wunder-ref]');"
                + "for(var i=0;i<old.length;i++)old[i].removeAttribute('data-wunder-ref');"
                + "var selector='a,button,input,textarea,select,summary,[role=\"button\"],[role=\"link\"],[role=\"checkbox\"],[role=\"radio\"],[role=\"tab\"],[role=\"menuitem\"],[contenteditable=\"true\"],[onclick]';"
                + "var all=document.querySelectorAll(selector);"
                + "function visible(el){try{var r=el.getBoundingClientRect();var s=getComputedStyle(el);"
                + "return r.width>2&&r.height>2&&s.visibility!=='hidden'&&s.display!=='none'&&parseFloat(s.opacity||'1')>0.05;}catch(e){return false;}}"
                + "function label(el){var t=el.getAttribute('aria-label')||el.getAttribute('placeholder')||el.getAttribute('alt')||el.getAttribute('title')||'';"
                + "if(!t){if(el.tagName==='SELECT'){var o=el.options[el.selectedIndex];t=(o&&(o.label||o.text||'').trim())||el.value||'';}"
                + "else if(el.tagName==='INPUT'||el.tagName==='TEXTAREA')t=el.value||'';else t=(el.innerText||'').trim();}"
                + "return (t||'').replace(/\\s+/g,' ').slice(0,120);}"
                + "function role(el){var tag=el.tagName.toLowerCase();var r=(el.getAttribute('role')||'').toLowerCase();"
                + "if(tag==='a')return 'link';"
                + "if(tag==='button'||r==='button')return 'button';"
                + "if(tag==='select')return 'combobox';"
                + "if(tag==='textarea'||el.isContentEditable)return 'textbox';"
                + "if(tag==='input'){var t=(el.type||'text').toLowerCase();"
                + "if(t==='checkbox')return 'checkbox';if(t==='radio')return 'radio';"
                + "if(t==='submit'||t==='button'||t==='reset')return 'button';return 'textbox';}"
                + "return r||'clickable';}"
                + "var lines=[];var n=0;"
                + "for(var i=0;i<all.length&&n<" + MAX_SNAPSHOT_NODES + ";i++){var el=all[i];if(!visible(el))continue;"
                + "n++;var ref='e'+n;el.setAttribute('data-wunder-ref',ref);"
                + "var line='- '+ref+' '+role(el)+' \"'+label(el).replace(/\"/g,'\\\\\"')+'\"';"
                + "if(el.tagName==='A'&&el.href)line+=' href=\"'+(el.href||'').slice(0,200)+'\"';"
                + "if(el.tagName==='INPUT')line+=' type='+(el.type||'text');"
                + "if(el.disabled)line+=' disabled';"
                + "lines.push(line);}"
                + "return JSON.stringify({url:location.href,title:document.title,text:lines.join('\\n')});"
                + "})()";
    }

    /** Escape one string as a JavaScript double-quoted literal. */
    static String quote(String value) {
        if (value == null) {
            return "\"\"";
        }
        StringBuilder out = new StringBuilder(value.length() + 2);
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\n':
                    out.append("\\n");
                    break;
                case '\r':
                    out.append("\\r");
                    break;
                case '\t':
                    out.append("\\t");
                    break;
                case '\b':
                    out.append("\\b");
                    break;
                case '\f':
                    out.append("\\f");
                    break;
                case '\u2028':
                    out.append("\\u2028");
                    break;
                case '\u2029':
                    out.append("\\u2029");
                    break;
                default:
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
            }
        }
        return out.append('"').toString();
    }
}
