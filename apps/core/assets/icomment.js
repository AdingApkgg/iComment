var iComment=(function(e){Object.defineProperty(e,Symbol.toStringTag,{value:`Module`});var t=globalThis,n=t.ShadowRoot&&(t.ShadyCSS===void 0||t.ShadyCSS.nativeShadow)&&`adoptedStyleSheets`in Document.prototype&&`replace`in CSSStyleSheet.prototype,r=Symbol(),i=new WeakMap,a=class{constructor(e,t,n){if(this._$cssResult$=!0,n!==r)throw Error("CSSResult is not constructable. Use `unsafeCSS` or `css` instead.");this.cssText=e,this.t=t}get styleSheet(){let e=this.o,t=this.t;if(n&&e===void 0){let n=t!==void 0&&t.length===1;n&&(e=i.get(t)),e===void 0&&((this.o=e=new CSSStyleSheet).replaceSync(this.cssText),n&&i.set(t,e))}return e}toString(){return this.cssText}},o=e=>new a(typeof e==`string`?e:e+``,void 0,r),s=(e,...t)=>new a(e.length===1?e[0]:t.reduce((t,n,r)=>t+(e=>{if(!0===e._$cssResult$)return e.cssText;if(typeof e==`number`)return e;throw Error(`Value passed to 'css' function must be a 'css' function result: `+e+`. Use 'unsafeCSS' to pass non-literal values, but take care to ensure page security.`)})(n)+e[r+1],e[0]),e,r),c=(e,r)=>{if(n)e.adoptedStyleSheets=r.map(e=>e instanceof CSSStyleSheet?e:e.styleSheet);else for(let n of r){let r=document.createElement(`style`),i=t.litNonce;i!==void 0&&r.setAttribute(`nonce`,i),r.textContent=n.cssText,e.appendChild(r)}},l=n?e=>e:e=>e instanceof CSSStyleSheet?(e=>{let t=``;for(let n of e.cssRules)t+=n.cssText;return o(t)})(e):e,{is:u,defineProperty:d,getOwnPropertyDescriptor:f,getOwnPropertyNames:p,getOwnPropertySymbols:m,getPrototypeOf:ee}=Object,h=globalThis,te=h.trustedTypes,ne=te?te.emptyScript:``,re=h.reactiveElementPolyfillSupport,g=(e,t)=>e,_={toAttribute(e,t){switch(t){case Boolean:e=e?ne:null;break;case Object:case Array:e=e==null?e:JSON.stringify(e)}return e},fromAttribute(e,t){let n=e;switch(t){case Boolean:n=e!==null;break;case Number:n=e===null?null:Number(e);break;case Object:case Array:try{n=JSON.parse(e)}catch{n=null}}return n}},ie=(e,t)=>!u(e,t),ae={attribute:!0,type:String,converter:_,reflect:!1,useDefault:!1,hasChanged:ie};Symbol.metadata??=Symbol(`metadata`),h.litPropertyMetadata??=new WeakMap;var v=class extends HTMLElement{static addInitializer(e){this._$Ei(),(this.l??=[]).push(e)}static get observedAttributes(){return this.finalize(),this._$Eh&&[...this._$Eh.keys()]}static createProperty(e,t=ae){if(t.state&&(t.attribute=!1),this._$Ei(),this.prototype.hasOwnProperty(e)&&((t=Object.create(t)).wrapped=!0),this.elementProperties.set(e,t),!t.noAccessor){let n=Symbol(),r=this.getPropertyDescriptor(e,n,t);r!==void 0&&d(this.prototype,e,r)}}static getPropertyDescriptor(e,t,n){let{get:r,set:i}=f(this.prototype,e)??{get(){return this[t]},set(e){this[t]=e}};return{get:r,set(t){let a=r?.call(this);i?.call(this,t),this.requestUpdate(e,a,n)},configurable:!0,enumerable:!0}}static getPropertyOptions(e){return this.elementProperties.get(e)??ae}static _$Ei(){if(this.hasOwnProperty(g(`elementProperties`)))return;let e=ee(this);e.finalize(),e.l!==void 0&&(this.l=[...e.l]),this.elementProperties=new Map(e.elementProperties)}static finalize(){if(this.hasOwnProperty(g(`finalized`)))return;if(this.finalized=!0,this._$Ei(),this.hasOwnProperty(g(`properties`))){let e=this.properties,t=[...p(e),...m(e)];for(let n of t)this.createProperty(n,e[n])}let e=this[Symbol.metadata];if(e!==null){let t=litPropertyMetadata.get(e);if(t!==void 0)for(let[e,n]of t)this.elementProperties.set(e,n)}this._$Eh=new Map;for(let[e,t]of this.elementProperties){let n=this._$Eu(e,t);n!==void 0&&this._$Eh.set(n,e)}this.elementStyles=this.finalizeStyles(this.styles)}static finalizeStyles(e){let t=[];if(Array.isArray(e)){let n=new Set(e.flat(1/0).reverse());for(let e of n)t.unshift(l(e))}else e!==void 0&&t.push(l(e));return t}static _$Eu(e,t){let n=t.attribute;return!1===n?void 0:typeof n==`string`?n:typeof e==`string`?e.toLowerCase():void 0}constructor(){super(),this._$Ep=void 0,this.isUpdatePending=!1,this.hasUpdated=!1,this._$Em=null,this._$Ev()}_$Ev(){this._$ES=new Promise(e=>this.enableUpdating=e),this._$AL=new Map,this._$E_(),this.requestUpdate(),this.constructor.l?.forEach(e=>e(this))}addController(e){(this._$EO??=new Set).add(e),this.renderRoot!==void 0&&this.isConnected&&e.hostConnected?.()}removeController(e){this._$EO?.delete(e)}_$E_(){let e=new Map,t=this.constructor.elementProperties;for(let n of t.keys())this.hasOwnProperty(n)&&(e.set(n,this[n]),delete this[n]);e.size>0&&(this._$Ep=e)}createRenderRoot(){let e=this.shadowRoot??this.attachShadow(this.constructor.shadowRootOptions);return c(e,this.constructor.elementStyles),e}connectedCallback(){this.renderRoot??=this.createRenderRoot(),this.enableUpdating(!0),this._$EO?.forEach(e=>e.hostConnected?.())}enableUpdating(e){}disconnectedCallback(){this._$EO?.forEach(e=>e.hostDisconnected?.())}attributeChangedCallback(e,t,n){this._$AK(e,n)}_$ET(e,t){let n=this.constructor.elementProperties.get(e),r=this.constructor._$Eu(e,n);if(r!==void 0&&!0===n.reflect){let i=(n.converter?.toAttribute===void 0?_:n.converter).toAttribute(t,n.type);this._$Em=e,i==null?this.removeAttribute(r):this.setAttribute(r,i),this._$Em=null}}_$AK(e,t){let n=this.constructor,r=n._$Eh.get(e);if(r!==void 0&&this._$Em!==r){let e=n.getPropertyOptions(r),i=typeof e.converter==`function`?{fromAttribute:e.converter}:e.converter?.fromAttribute===void 0?_:e.converter;this._$Em=r;let a=i.fromAttribute(t,e.type);this[r]=a??this._$Ej?.get(r)??a,this._$Em=null}}requestUpdate(e,t,n,r=!1,i){if(e!==void 0){let a=this.constructor;if(!1===r&&(i=this[e]),n??=a.getPropertyOptions(e),!((n.hasChanged??ie)(i,t)||n.useDefault&&n.reflect&&i===this._$Ej?.get(e)&&!this.hasAttribute(a._$Eu(e,n))))return;this.C(e,t,n)}!1===this.isUpdatePending&&(this._$ES=this._$EP())}C(e,t,{useDefault:n,reflect:r,wrapped:i},a){n&&!(this._$Ej??=new Map).has(e)&&(this._$Ej.set(e,a??t??this[e]),!0!==i||a!==void 0)||(this._$AL.has(e)||(this.hasUpdated||n||(t=void 0),this._$AL.set(e,t)),!0===r&&this._$Em!==e&&(this._$Eq??=new Set).add(e))}async _$EP(){this.isUpdatePending=!0;try{await this._$ES}catch(e){Promise.reject(e)}let e=this.scheduleUpdate();return e!=null&&await e,!this.isUpdatePending}scheduleUpdate(){return this.performUpdate()}performUpdate(){if(!this.isUpdatePending)return;if(!this.hasUpdated){if(this.renderRoot??=this.createRenderRoot(),this._$Ep){for(let[e,t]of this._$Ep)this[e]=t;this._$Ep=void 0}let e=this.constructor.elementProperties;if(e.size>0)for(let[t,n]of e){let{wrapped:e}=n,r=this[t];!0!==e||this._$AL.has(t)||r===void 0||this.C(t,void 0,n,r)}}let e=!1,t=this._$AL;try{e=this.shouldUpdate(t),e?(this.willUpdate(t),this._$EO?.forEach(e=>e.hostUpdate?.()),this.update(t)):this._$EM()}catch(t){throw e=!1,this._$EM(),t}e&&this._$AE(t)}willUpdate(e){}_$AE(e){this._$EO?.forEach(e=>e.hostUpdated?.()),this.hasUpdated||(this.hasUpdated=!0,this.firstUpdated(e)),this.updated(e)}_$EM(){this._$AL=new Map,this.isUpdatePending=!1}get updateComplete(){return this.getUpdateComplete()}getUpdateComplete(){return this._$ES}shouldUpdate(e){return!0}update(e){this._$Eq&&=this._$Eq.forEach(e=>this._$ET(e,this[e])),this._$EM()}updated(e){}firstUpdated(e){}};v.elementStyles=[],v.shadowRootOptions={mode:`open`},v[g(`elementProperties`)]=new Map,v[g(`finalized`)]=new Map,re?.({ReactiveElement:v}),(h.reactiveElementVersions??=[]).push(`2.1.2`);var y=globalThis,oe=e=>e,b=y.trustedTypes,se=b?b.createPolicy(`lit-html`,{createHTML:e=>e}):void 0,x=`$lit$`,S=`lit$${Math.random().toFixed(9).slice(2)}$`,C=`?`+S,ce=`<${C}>`,w=document,T=()=>w.createComment(``),E=e=>e===null||typeof e!=`object`&&typeof e!=`function`,D=Array.isArray,le=e=>D(e)||typeof e?.[Symbol.iterator]==`function`,O=`[ 	
\f\r]`,k=/<(?:(!--|\/[^a-zA-Z])|(\/?[a-zA-Z][^>\s]*)|(\/?$))/g,ue=/-->/g,A=/>/g,j=RegExp(`>|${O}(?:([^\\s"'>=/]+)(${O}*=${O}*(?:[^ \t\n\f\r"'\`<>=]|("|')|))|$)`,`g`),M=/'/g,N=/"/g,de=/^(?:script|style|textarea|title)$/i,P=(e=>(t,...n)=>({_$litType$:e,strings:t,values:n}))(1),F=Symbol.for(`lit-noChange`),I=Symbol.for(`lit-nothing`),fe=new WeakMap,L=w.createTreeWalker(w,129);function pe(e,t){if(!D(e)||!e.hasOwnProperty(`raw`))throw Error(`invalid template strings array`);return se===void 0?t:se.createHTML(t)}var me=(e,t)=>{let n=e.length-1,r=[],i,a=t===2?`<svg>`:t===3?`<math>`:``,o=k;for(let t=0;t<n;t++){let n=e[t],s,c,l=-1,u=0;for(;u<n.length&&(o.lastIndex=u,c=o.exec(n),c!==null);)u=o.lastIndex,o===k?c[1]===`!--`?o=ue:c[1]===void 0?c[2]===void 0?c[3]!==void 0&&(o=j):(de.test(c[2])&&(i=RegExp(`</`+c[2],`g`)),o=j):o=A:o===j?c[0]===`>`?(o=i??k,l=-1):c[1]===void 0?l=-2:(l=o.lastIndex-c[2].length,s=c[1],o=c[3]===void 0?j:c[3]===`"`?N:M):o===N||o===M?o=j:o===ue||o===A?o=k:(o=j,i=void 0);let d=o===j&&e[t+1].startsWith(`/>`)?` `:``;a+=o===k?n+ce:l>=0?(r.push(s),n.slice(0,l)+x+n.slice(l)+S+d):n+S+(l===-2?t:d)}return[pe(e,a+(e[n]||`<?>`)+(t===2?`</svg>`:t===3?`</math>`:``)),r]},R=class e{constructor({strings:t,_$litType$:n},r){let i;this.parts=[];let a=0,o=0,s=t.length-1,c=this.parts,[l,u]=me(t,n);if(this.el=e.createElement(l,r),L.currentNode=this.el.content,n===2||n===3){let e=this.el.content.firstChild;e.replaceWith(...e.childNodes)}for(;(i=L.nextNode())!==null&&c.length<s;){if(i.nodeType===1){if(i.hasAttributes())for(let e of i.getAttributeNames())if(e.endsWith(x)){let t=u[o++],n=i.getAttribute(e).split(S),r=/([.?@])?(.*)/.exec(t);c.push({type:1,index:a,name:r[2],strings:n,ctor:r[1]===`.`?U:r[1]===`?`?he:r[1]===`@`?ge:H}),i.removeAttribute(e)}else e.startsWith(S)&&(c.push({type:6,index:a}),i.removeAttribute(e));if(de.test(i.tagName)){let e=i.textContent.split(S),t=e.length-1;if(t>0){i.textContent=b?b.emptyScript:``;for(let n=0;n<t;n++)i.append(e[n],T()),L.nextNode(),c.push({type:2,index:++a});i.append(e[t],T())}}}else if(i.nodeType===8){if(i.data===C)c.push({type:2,index:a});else{let e=-1;for(;(e=i.data.indexOf(S,e+1))!==-1;)c.push({type:7,index:a}),e+=S.length-1}}a++}}static createElement(e,t){let n=w.createElement(`template`);return n.innerHTML=e,n}};function z(e,t,n=e,r){if(t===F)return t;let i=r===void 0?n._$Cl:n._$Co?.[r],a=E(t)?void 0:t._$litDirective$;return i?.constructor!==a&&(i?._$AO?.(!1),a===void 0?i=void 0:(i=new a(e),i._$AT(e,n,r)),r===void 0?n._$Cl=i:(n._$Co??=[])[r]=i),i!==void 0&&(t=z(e,i._$AS(e,t.values),i,r)),t}var B=class{constructor(e,t){this._$AV=[],this._$AN=void 0,this._$AD=e,this._$AM=t}get parentNode(){return this._$AM.parentNode}get _$AU(){return this._$AM._$AU}u(e){let{el:{content:t},parts:n}=this._$AD,r=(e?.creationScope??w).importNode(t,!0);L.currentNode=r;let i=L.nextNode(),a=0,o=0,s=n[0];for(;s!==void 0;){if(a===s.index){let t;s.type===2?t=new V(i,i.nextSibling,this,e):s.type===1?t=new s.ctor(i,s.name,s.strings,this,e):s.type===6&&(t=new _e(i,this,e)),this._$AV.push(t),s=n[++o]}a!==s?.index&&(i=L.nextNode(),a++)}return L.currentNode=w,r}p(e){let t=0;for(let n of this._$AV)n!==void 0&&(n.strings===void 0?n._$AI(e[t]):(n._$AI(e,n,t),t+=n.strings.length-2)),t++}},V=class e{get _$AU(){return this._$AM?._$AU??this._$Cv}constructor(e,t,n,r){this.type=2,this._$AH=I,this._$AN=void 0,this._$AA=e,this._$AB=t,this._$AM=n,this.options=r,this._$Cv=r?.isConnected??!0}get parentNode(){let e=this._$AA.parentNode,t=this._$AM;return t!==void 0&&e?.nodeType===11&&(e=t.parentNode),e}get startNode(){return this._$AA}get endNode(){return this._$AB}_$AI(e,t=this){e=z(this,e,t),E(e)?e===I||e==null||e===``?(this._$AH!==I&&this._$AR(),this._$AH=I):e!==this._$AH&&e!==F&&this._(e):e._$litType$===void 0?e.nodeType===void 0?le(e)?this.k(e):this._(e):this.T(e):this.$(e)}O(e){return this._$AA.parentNode.insertBefore(e,this._$AB)}T(e){this._$AH!==e&&(this._$AR(),this._$AH=this.O(e))}_(e){this._$AH!==I&&E(this._$AH)?this._$AA.nextSibling.data=e:this.T(w.createTextNode(e)),this._$AH=e}$(e){let{values:t,_$litType$:n}=e,r=typeof n==`number`?this._$AC(e):(n.el===void 0&&(n.el=R.createElement(pe(n.h,n.h[0]),this.options)),n);if(this._$AH?._$AD===r)this._$AH.p(t);else{let e=new B(r,this),n=e.u(this.options);e.p(t),this.T(n),this._$AH=e}}_$AC(e){let t=fe.get(e.strings);return t===void 0&&fe.set(e.strings,t=new R(e)),t}k(t){D(this._$AH)||(this._$AH=[],this._$AR());let n=this._$AH,r,i=0;for(let a of t)i===n.length?n.push(r=new e(this.O(T()),this.O(T()),this,this.options)):r=n[i],r._$AI(a),i++;i<n.length&&(this._$AR(r&&r._$AB.nextSibling,i),n.length=i)}_$AR(e=this._$AA.nextSibling,t){for(this._$AP?.(!1,!0,t);e!==this._$AB;){let t=oe(e).nextSibling;oe(e).remove(),e=t}}setConnected(e){this._$AM===void 0&&(this._$Cv=e,this._$AP?.(e))}},H=class{get tagName(){return this.element.tagName}get _$AU(){return this._$AM._$AU}constructor(e,t,n,r,i){this.type=1,this._$AH=I,this._$AN=void 0,this.element=e,this.name=t,this._$AM=r,this.options=i,n.length>2||n[0]!==``||n[1]!==``?(this._$AH=Array(n.length-1).fill(new String),this.strings=n):this._$AH=I}_$AI(e,t=this,n,r){let i=this.strings,a=!1;if(i===void 0)e=z(this,e,t,0),a=!E(e)||e!==this._$AH&&e!==F,a&&(this._$AH=e);else{let r=e,o,s;for(e=i[0],o=0;o<i.length-1;o++)s=z(this,r[n+o],t,o),s===F&&(s=this._$AH[o]),a||=!E(s)||s!==this._$AH[o],s===I?e=I:e!==I&&(e+=(s??``)+i[o+1]),this._$AH[o]=s}a&&!r&&this.j(e)}j(e){e===I?this.element.removeAttribute(this.name):this.element.setAttribute(this.name,e??``)}},U=class extends H{constructor(){super(...arguments),this.type=3}j(e){this.element[this.name]=e===I?void 0:e}},he=class extends H{constructor(){super(...arguments),this.type=4}j(e){this.element.toggleAttribute(this.name,!!e&&e!==I)}},ge=class extends H{constructor(e,t,n,r,i){super(e,t,n,r,i),this.type=5}_$AI(e,t=this){if((e=z(this,e,t,0)??I)===F)return;let n=this._$AH,r=e===I&&n!==I||e.capture!==n.capture||e.once!==n.once||e.passive!==n.passive,i=e!==I&&(n===I||r);r&&this.element.removeEventListener(this.name,this,n),i&&this.element.addEventListener(this.name,this,e),this._$AH=e}handleEvent(e){typeof this._$AH==`function`?this._$AH.call(this.options?.host??this.element,e):this._$AH.handleEvent(e)}},_e=class{constructor(e,t,n){this.element=e,this.type=6,this._$AN=void 0,this._$AM=t,this.options=n}get _$AU(){return this._$AM._$AU}_$AI(e){z(this,e)}},ve={M:x,P:S,A:C,C:1,L:me,R:B,D:le,V:z,I:V,H,N:he,U:ge,B:U,F:_e},ye=y.litHtmlPolyfillSupport;ye?.(R,V),(y.litHtmlVersions??=[]).push(`3.3.3`);var be=(e,t,n)=>{let r=n?.renderBefore??t,i=r._$litPart$;if(i===void 0){let e=n?.renderBefore??null;r._$litPart$=i=new V(t.insertBefore(T(),e),e,void 0,n??{})}return i._$AI(e),i},W=globalThis,G=class extends v{constructor(){super(...arguments),this.renderOptions={host:this},this._$Do=void 0}createRenderRoot(){let e=super.createRenderRoot();return this.renderOptions.renderBefore??=e.firstChild,e}update(e){let t=this.render();this.hasUpdated||(this.renderOptions.isConnected=this.isConnected),super.update(e),this._$Do=be(t,this.renderRoot,this.renderOptions)}connectedCallback(){super.connectedCallback(),this._$Do?.setConnected(!0)}disconnectedCallback(){super.disconnectedCallback(),this._$Do?.setConnected(!1)}render(){return F}};G._$litElement$=!0,G.finalized=!0,W.litElementHydrateSupport?.({LitElement:G});var xe=W.litElementPolyfillSupport;xe?.({LitElement:G}),(W.litElementVersions??=[]).push(`4.2.2`);var K={ATTRIBUTE:1,CHILD:2,PROPERTY:3,BOOLEAN_ATTRIBUTE:4,EVENT:5,ELEMENT:6},Se=e=>(...t)=>({_$litDirective$:e,values:t}),Ce=class{constructor(e){}get _$AU(){return this._$AM._$AU}_$AT(e,t,n){this._$Ct=e,this._$AM=t,this._$Ci=n}_$AS(e,t){return this.update(e,t)}update(e,t){return this.render(...t)}},{I:we}=ve,Te=e=>e,Ee=e=>e.strings===void 0,De=()=>document.createComment(``),q=(e,t,n)=>{let r=e._$AA.parentNode,i=t===void 0?e._$AB:t._$AA;if(n===void 0)n=new we(r.insertBefore(De(),i),r.insertBefore(De(),i),e,e.options);else{let t=n._$AB.nextSibling,a=n._$AM,o=a!==e;if(o){let t;n._$AQ?.(e),n._$AM=e,n._$AP!==void 0&&(t=e._$AU)!==a._$AU&&n._$AP(t)}if(t!==i||o){let e=n._$AA;for(;e!==t;){let t=Te(e).nextSibling;Te(r).insertBefore(e,i),e=t}}}return n},J=(e,t,n=e)=>(e._$AI(t,n),e),Oe={},ke=(e,t=Oe)=>e._$AH=t,Ae=e=>e._$AH,Y=e=>{e._$AR(),e._$AA.remove()},X=Se(class extends Ce{constructor(e){if(super(e),e.type!==K.PROPERTY&&e.type!==K.ATTRIBUTE&&e.type!==K.BOOLEAN_ATTRIBUTE)throw Error("The `live` directive is not allowed on child or event bindings");if(!Ee(e))throw Error("`live` bindings can only contain a single expression")}render(e){return e}update(e,[t]){if(t===F||t===I)return t;let n=e.element,r=e.name;if(e.type===K.PROPERTY){if(t===n[r])return F}else if(e.type===K.BOOLEAN_ATTRIBUTE){if(!!t===n.hasAttribute(r))return F}else if(e.type===K.ATTRIBUTE&&n.getAttribute(r)===t+``)return F;return ke(e),t}}),je=(e,t,n)=>{let r=new Map;for(let i=t;i<=n;i++)r.set(e[i],i);return r},Me=Se(class extends Ce{constructor(e){if(super(e),e.type!==K.CHILD)throw Error(`repeat() can only be used in text expressions`)}dt(e,t,n){let r;n===void 0?n=t:t!==void 0&&(r=t);let i=[],a=[],o=0;for(let t of e)i[o]=r?r(t,o):o,a[o]=n(t,o),o++;return{values:a,keys:i}}render(e,t,n){return this.dt(e,t,n).values}update(e,[t,n,r]){let i=Ae(e),{values:a,keys:o}=this.dt(t,n,r);if(!Array.isArray(i))return this.ut=o,a;let s=this.ut??=[],c=[],l,u,d=0,f=i.length-1,p=0,m=a.length-1;for(;d<=f&&p<=m;)if(i[d]===null)d++;else if(i[f]===null)f--;else if(s[d]===o[p])c[p]=J(i[d],a[p]),d++,p++;else if(s[f]===o[m])c[m]=J(i[f],a[m]),f--,m--;else if(s[d]===o[m])c[m]=J(i[d],a[m]),q(e,c[m+1],i[d]),d++,m--;else if(s[f]===o[p])c[p]=J(i[f],a[p]),q(e,i[d],i[f]),f--,p++;else if(l===void 0&&(l=je(o,p,m),u=je(s,d,f)),l.has(s[d])){if(l.has(s[f])){let t=u.get(o[p]),n=t===void 0?null:i[t];if(n===null){let t=q(e,i[d]);J(t,a[p]),c[p]=t}else c[p]=J(n,a[p]),q(e,i[d],n),i[t]=null;p++}else Y(i[f]),f--}else Y(i[d]),d++;for(;p<=m;){let t=q(e,c[m+1]);J(t,a[p]),c[p++]=t}for(;d<=f;){let e=i[d++];e!==null&&Y(e)}return this.ut=o,ke(e,c),F}}),Z=class extends Error{constructor(e,t,n,r={}){super(t),this.code=e,this.status=n,this.fields=r}},Ne=class{constructor(e,t){this.base=e,this.siteKey=t}async call(e,t={}){let n=await fetch(`${this.base}${e}`,{...t,headers:{"content-type":`application/json`,"x-site-key":this.siteKey,...t.headers},credentials:`omit`});if(n.status===204)return;let r=await n.json().catch(()=>null);if(!n.ok){let e=r?.code??`unknown_error`,t=r?.fields??{};throw new Z(e,r?.message??n.statusText,n.status,t)}return r}meta(e){return this.call(`/site?path=${encodeURIComponent(e)}`,{method:`GET`})}page(e,t){let n=new URLSearchParams({path:e,limit:`40`});return t&&n.set(`cursor`,t),this.call(`/comments?${n}`,{method:`GET`})}post(e,t){return this.call(`/comments`,{method:`POST`,body:JSON.stringify({path:e,...t})})}react(e,t,n){return this.call(`/comments/${e}/reactions/${t}`,{method:n?`POST`:`DELETE`})}},Pe=40,Fe=[[`up`,`👍`,`赞`],[`heart`,`❤️`,`喜欢`],[`haha`,`😄`,`好笑`],[`thinking`,`🤔`,`想想`],[`party`,`🎉`,`庆祝`]],Ie=document.currentScript?.src??``,Le=()=>({meta:null,comments:[],next:null,total:0,loading:!0,error:null,replyTo:null,name:``,email:``,body:``,sending:!1,notice:null,reacting:null});function Re(e){let t=new Map([[null,[]]]);for(let n of e){let e=n.parent_id??null,r=t.get(e)??[];r.push(n),t.set(e,r)}return t}var Q=class extends G{constructor(...e){super(...e),this.siteKey=``,this.apiBase=``,this.path=``,this.theme=``,this.maxDepth=0,this.api=null,this._view=Le(),this.turnstileToken=null,this.draftKey=null,this.onTurnstile=e=>{this.turnstileToken=e.detail}}static{this.properties={siteKey:{type:String,attribute:`site-key`},apiBase:{type:String,attribute:`api-base`},path:{type:String,attribute:`path`},theme:{type:String,attribute:`theme`},maxDepth:{type:Number,attribute:`max-depth`},_view:{state:!0}}}static{this.styles=s`
    :host {
      --ic-fg: #10131a;
      --ic-muted: #646d7c;
      --ic-line: #e2e6ee;
      --ic-bg: #fff;
      --ic-brand: #10131a;
      --ic-ok: #0b7a4b;
      --ic-bad: #b42318;
      display: block;
      color: var(--ic-fg);
      font: 15px/1.65 ui-sans-serif, system-ui, -apple-system, "Segoe UI", "PingFang SC",
        "Noto Sans CJK SC", sans-serif;
    }
    :host([hidden]) {
      display: none;
    }
    :host {
      @media (prefers-color-scheme: dark) {
        --ic-fg: #e8ecf3;
        --ic-muted: #98a2b3;
        --ic-line: #2a313d;
        --ic-bg: #10131a;
        --ic-brand: #fff;
      }
    }
    :host([theme="dark"]) {
      --ic-fg: #e8ecf3;
      --ic-muted: #98a2b3;
      --ic-line: #2a313d;
      --ic-bg: #10131a;
      --ic-brand: #fff;
    }
    :host([theme="light"]) {
      --ic-fg: #10131a;
      --ic-muted: #646d7c;
      --ic-line: #e2e6ee;
      --ic-bg: #fff;
      --ic-brand: #10131a;
      --ic-ok: #0b7a4b;
      --ic-bad: #b42318;
    }
    @media (prefers-reduced-motion: reduce) {
      :host {
        scroll-behavior: auto;
      }
    }
    /* 只给读屏用的标签，视觉上隐藏。 */
    .sr {
      position: absolute;
      width: 1px;
      height: 1px;
      margin: -1px;
      padding: 0;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    h2 {
      margin: 0 0 0.75rem;
      font-size: 1rem;
      font-weight: 650;
      letter-spacing: -0.01em;
    }
    .count {
      color: var(--ic-muted);
      font-weight: 400;
      font-variant-numeric: tabular-nums;
    }
    fieldset {
      border: 1px solid var(--ic-line);
      border-radius: 10px;
      margin: 0 0 1.5rem;
      padding: 0.8rem;
      display: grid;
      gap: 0.6rem;
    }
    input,
    textarea {
      width: 100%;
      box-sizing: border-box;
      font: inherit;
      color: inherit;
      background: var(--ic-bg);
      border: 1px solid var(--ic-line);
      border-radius: 7px;
      padding: 0.5rem 0.6rem;
    }
    textarea {
      min-height: 6.5rem;
      resize: vertical;
    }
    .pair {
      display: grid;
      gap: 0.6rem;
      grid-template-columns: 1fr 1fr;
    }
    @media (max-width: 520px) {
      .pair {
        grid-template-columns: 1fr;
      }
    }
    button {
      font: inherit;
      font-weight: 600;
      cursor: pointer;
      border: 0;
      border-radius: 7px;
      padding: 0.45rem 0.9rem;
      background: var(--ic-brand);
      color: var(--ic-bg);
    }
    button[disabled] {
      opacity: 0.55;
      cursor: progress;
    }
    .reacts {
      display: flex;
      gap: 0.3rem;
      flex-wrap: wrap;
      margin-top: 0.4rem;
    }
    .reacts button {
      font-size: 0.8rem;
      padding: 0.1rem 0.45rem;
      border-radius: 99px;
      border: 1px solid var(--ic-line);
      background: none;
      color: var(--ic-muted);
      min-height: 1.8rem;
    }
    .reacts button[aria-pressed="true"] {
      border-color: var(--ic-accent);
      color: var(--ic-accent);
      font-weight: 600;
    }
    .reacts button[disabled] {
      cursor: progress;
    }
    button.link {
      background: none;
      color: var(--ic-muted);
      padding: 0;
      font-weight: 500;
      text-decoration: underline;
      text-underline-offset: 2px;
    }
    .row {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: 0.5rem;
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 1rem;
    }
    li {
      border-top: 1px solid var(--ic-line);
      padding-top: 1rem;
    }
    .children {
      margin-top: 0.8rem;
      padding-left: 0.9rem;
      border-left: 2px solid var(--ic-line);
      display: grid;
      gap: 0.9rem;
    }
    .head {
      display: flex;
      gap: 0.5rem;
      align-items: center;
      font-size: 0.85rem;
      color: var(--ic-muted);
    }
    .head b {
      color: var(--ic-fg);
      font-weight: 600;
    }
    .avatar {
      width: 22px;
      height: 22px;
      border-radius: 50%;
      object-fit: cover;
      background: var(--ic-line);
    }
    .body {
      margin: 0.35rem 0 0;
      overflow-wrap: anywhere;
    }
    .body :where(pre) {
      overflow-x: auto;
      background: color-mix(in oklab, var(--ic-line) 40%, transparent);
      padding: 0.6rem;
      border-radius: 6px;
    }
    .badges {
      display: flex;
      gap: 0.35rem;
      flex-wrap: wrap;
      margin-top: 0.3rem;
    }
    .badge {
      font-size: 0.72rem;
      border: 1px solid var(--ic-line);
      border-radius: 99px;
      padding: 0.05rem 0.45rem;
      color: var(--ic-muted);
    }
    .notice,
    .error {
      font-size: 0.85rem;
      border-radius: 7px;
      padding: 0.5rem 0.7rem;
      margin-bottom: 0.8rem;
    }
    .notice {
      background: color-mix(in oklab, var(--ic-ok) 12%, transparent);
      color: var(--ic-ok);
    }
    .error {
      background: color-mix(in oklab, var(--ic-bad) 10%, transparent);
      color: var(--ic-bad);
    }
    .muted {
      color: var(--ic-muted);
    }
    .reply {
      font-size: 0.85rem;
      color: var(--ic-muted);
      margin-bottom: 0.5rem;
    }
    footer {
      display: flex;
      justify-content: center;
      padding: 1rem 0 0;
    }
    a {
      color: inherit;
      text-underline-offset: 2px;
    }
  `}connectedCallback(){super.connectedCallback(),this.api=new Ne(this.apiBase||this.deriveBase(),this.siteKey),this.load()}deriveBase(){return new URL(`/api/v1`,Ie||location.href).href}url(){return this.path||location.pathname+location.search}async load(e=null){if(this.api){this._view={...this._view,loading:!0,error:null};try{let[t,n]=await Promise.all([this._view.meta??this.api.meta(this.url()),this.api.page(this.url(),e)]);this._view={...this._view,meta:t,comments:e?[...this._view.comments,...n.comments]:n.comments,next:n.next_cursor,total:n.total,loading:!1},this.maybeMountTurnstile()}catch(e){this._view={...this._view,loading:!1,error:Be(e)}}}}maybeMountTurnstile(){let e=this._view.meta;if(!e?.turnstile_site_key||e.challenge_mode!==`always`||document.querySelector(`script[data-ic-turnstile]`))return;document.addEventListener(`ic:turnstile`,this.onTurnstile,{once:!0});let t=document.createElement(`script`);t.src=`https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit`,t.async=!0,t.dataset.icTurnstile=`1`,t.onload=()=>{let t=window,n=this.renderRoot.querySelector(`.turnstile`);t.turnstile&&n&&t.turnstile.render(n,{sitekey:e.turnstile_site_key,callback:e=>{this.turnstileToken=e},"expired-callback":()=>{this.turnstileToken=null},theme:window.matchMedia(`(prefers-color-scheme: dark)`).matches?`dark`:`light`})},document.head.append(t)}async submit(e){if(e.preventDefault(),!this.api||this._view.sending)return;let t=this._view.body.trim();if(!t)this._view={...this._view,error:`内容不能为空`};else if(this._view.meta?.challenge_mode===`always`&&!this.turnstileToken)this._view={...this._view,error:`请先完成人机验证`};else{this._view={...this._view,sending:!0,error:null,notice:null},this.draftKey??=crypto.randomUUID();try{let e=await this.api.post(this.url(),{body_md:t,parent_id:this._view.replyTo?.id??null,author_name:this._view.name.trim()||null,author_email:this._view.email.trim()||null,challenge_token:this.turnstileToken,idempotency_key:this.draftKey});this._view={...this._view,sending:!1,body:``,replyTo:null,notice:e.message??(e.created?`已发布`:`已收到，审核后显示`)},this.turnstileToken=null,this.draftKey=null,e.created&&await this.load()}catch(e){let t=e instanceof Z&&e.status===429?`发得太快了，稍等一会儿`:Be(e);this._view={...this._view,sending:!1,error:t}}}}async react(e,t){if(this._view.reacting||!this.api)return;let n=!e.reactions.mine.includes(t);this._view={...this._view,reacting:`${e.id}:${t}`};try{let r=await this.api.react(e.id,t,n);this._view={...this._view,reacting:null,comments:this._view.comments.map(t=>t.id===e.id?{...t,reactions:r}:t)}}catch(e){this._view={...this._view,reacting:null,error:e instanceof Z?e.message:`表情没投上，再试一次`}}}reactions(e){let t=([t,n,r])=>{let i=e.reactions.counts[t]??0,a=e.reactions.mine.includes(t),o=this._view.reacting===`${e.id}:${t}`;return P`<button
        type="button"
        aria-pressed=${a?`true`:`false`}
        aria-label=${`${r}，${i} 人`}
        ?disabled=${o}
        @click=${()=>this.react(e,t)}
      >${n}${i?P`<span class="count">${i}</span>`:I}</button>`},n=Fe.filter(([t])=>(e.reactions.counts[t]??0)>0||e.reactions.mine.includes(t)),r=Fe.filter(([e])=>!n.some(([t])=>t===e)),i=r.length>0?P`<details class="picker">
            <summary class="link" aria-label="加个表情">＋</summary>
            <div class="reacts">${r.map(t)}</div>
          </details>`:I;return n.length===0?i:P`<div class="reacts">${n.map(t)}${i}</div>`}reply(e){this._view={...this._view,replyTo:e,error:null},this.renderRoot.querySelector(`textarea`)?.focus()}renderComment(e,t,n,r){let i=t.get(e.id)??[],a=[];return e.status===`pending`&&a.push(`待审`),e.identity_unverified&&a.push(`未验证`),P`<li class="comment">
      <div class="head">
        ${e.author_avatar?P`<img class="avatar" src=${e.author_avatar} alt="" loading="lazy" />`:I}
        <b>${e.author_name??`匿名`}</b>
        <time datetime=${e.created_at}>${Ve(e.created_at)}</time>
        <button class="link" type="button" @click=${()=>this.reply(e)}>回复</button>
      </div>
      <div class="body">${ze(e.body_html)}</div>
      ${this.reactions(e)}
      ${a.length?P`<div class="badges">${a.map(e=>P`<span class="badge">${e}</span>`)}</div>`:I}
      ${i.length&&n<r?P`<div class="children">
            ${Me(i,e=>e.id,e=>this.renderComment(e,t,n+1,r))}
          </div>`:I}
    </li>`}failed(e){let t=this.url().replace(/^[^:]+:\/\/[^/]+/,``);return P`
      <p class="error">${e}</p>
      <p class="muted"><a href=${`${this.deriveOrigin()}/embed/${this.siteKey}${t}`} target="_blank" rel="noopener">在新页面读评论</a></p>
    `}deriveOrigin(){try{return new URL(this.apiBase||this.deriveBase()).origin}catch{return location.origin}}render(){let e=this._view,t=e.meta;if(!t&&e.loading)return P`<p class="muted">加载评论…</p>`;if(!t&&e.error)return this.failed(e.error);let n=Re(e.comments),r=n.get(null)??[],i=this.maxDepth||t?.max_depth||6;return P`
      <h2>
        ${e.total} 条评论
        ${t?.require_approval?P`<span class="count">（先审后发）</span>`:I}
      </h2>
      <div aria-live="polite">
        ${e.notice?P`<p class="notice">${e.notice}</p>`:I}
        ${e.error?P`<p class="error">${e.error}</p>`:I}
      </div>

      <form @submit=${this.submit}>
        <fieldset>
          ${e.replyTo?P`<div class="reply">
                回复 ${e.replyTo.author_name??`匿名`}
                <button class="link" type="button" @click=${()=>this._view={...e,replyTo:null}}>
                  取消
                </button>
              </div>`:I}
          <div class="pair">
            <label>
              <span class="sr">昵称</span>
              <input
                placeholder="昵称（可选）"
                autocomplete="nickname"
                .value=${X(e.name)}
                @input=${e=>this._view={...this._view,name:e.target.value}}
              />
            </label>
            <label>
              <span class="sr">邮箱（不公开）</span>
              <input
                type="email"
                placeholder="邮箱（可选，不公开）"
                autocomplete="email"
                .value=${X(e.email)}
                @input=${e=>this._view={...this._view,email:e.target.value}}
              />
            </label>
          </div>
          <label>
          <span class="sr">评论内容，支持 Markdown</span>
          <textarea
            placeholder="说点什么… 支持 Markdown"
            maxlength=${t?.max_body_bytes??8192}
            .value=${X(e.body)}
            @input=${e=>this._view={...this._view,body:e.target.value}}
          ></textarea>
          </label>
          ${t?.challenge_mode===`always`?P`<div class="turnstile"></div>`:I}
          <div class="row">
            <span class="muted">${e.body.length} 字</span>
            <button ?disabled=${e.sending||e.loading} type="submit">
              ${e.sending?`发送中…`:`发表`}
            </button>
          </div>
        </fieldset>
      </form>

      <ul ?aria-busy=${e.loading}>
        ${Me(r,e=>e.id,e=>this.renderComment(e,n,0,i))}
      </ul>
      ${r.length===0&&!e.loading?P`<p class="muted">还没有评论，来说第一句。</p>`:I}
      ${e.next?P`<footer>
            <button type="button" class="link" @click=${()=>this.load(e.next)}>
              加载更多（${Math.min(Pe,e.total-e.comments.length)}）
            </button>
          </footer>`:I}
    `}};function ze(e){let t=new DOMParser().parseFromString(e,`text/html`),n=document.createDocumentFragment();for(let e of t.body.childNodes)n.append(e.cloneNode(!0));return n}function Be(e){return e instanceof Z?Object.values(e.fields)[0]??e.message:e instanceof Error?e.message:`请求失败`}function Ve(e){let t=new Date(e).getTime(),n=Math.round((Date.now()-t)/1e3);return Number.isNaN(n)?e:n<60?`刚刚`:n<3600?`${Math.floor(n/60)} 分钟前`:n<86400?`${Math.floor(n/3600)} 小时前`:n<604800?`${Math.floor(n/86400)} 天前`:new Date(e).toLocaleDateString(`zh-CN`)}customElements.get(`ic-comment`)||customElements.define(`ic-comment`,Q),customElements.get(`icomment-widget`)||customElements.define(`icomment-widget`,class extends Q{});var $=document.currentScript,He=$?.dataset.siteKey;if(He&&!document.querySelector(`ic-comment`)){let e=document.createElement(`ic-comment`);e.setAttribute(`site-key`,He),$?.dataset.apiBase&&e.setAttribute(`api-base`,$.dataset.apiBase),$?.after(e)}return e.CommentWidget=Q,e})({});
//# sourceMappingURL=icomment.js.map