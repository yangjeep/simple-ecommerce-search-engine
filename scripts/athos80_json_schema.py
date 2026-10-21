#!/usr/bin/env python3
"""Generate readable draft-2020-12 input schemas matching the v1.1 Rust DTOs."""
import json, pathlib
root=pathlib.Path(__file__).resolve().parents[1]/'crates/athos80-eval/schema'
S={'type':'string'}; I={'type':'integer','minimum':-(2**63),'maximum':2**63-1}; U={'type':'integer','minimum':0,'maximum':2**64-1}; U32={'type':'integer','minimum':0,'maximum':2**32-1}; I32={'type':'integer','minimum':-(2**31),'maximum':2**31-1}; N={'type':'number'}; B={'type':'boolean'}
def ref(n):return {'$ref':'#/$defs/'+n}
def arr(v):return {'type':'array','items':v}
def obj(p,required=None):return {'type':'object','properties':p,'required':list(p) if required is None else required,'additionalProperties':False}
def tag(name,fields={},key='type',required=None):return obj({key:{'const':name},**fields},None if required is None else [key]+required)
def union(*vs):return {'oneOf':list(vs)}
def enum(*vs):return {'enum':list(vs)}
d={}
d['Value']=union(*(tag(k,{'value':v}) for k,v in [('enum',S),('multi_enum',arr(S)),('boolean',B),('numeric',N),('text',S)]))
d['Attributes']={'type':'object','additionalProperties':ref('Value')}
d['Filter']=union(tag('enum',{'attribute':S,'value':S}),tag('multi_enum_contains',{'attribute':S,'value':S}),tag('boolean',{'attribute':S,'value':B}),tag('numeric',{'attribute':S,'op':enum('eq','lt','lte','gt','gte'),'value':N}),tag('text_contains',{'attribute':S,'value':S}),*(tag(k,{'value':U32}) for k in ['brand','category','product_type']),*(tag(k,{'value':I}) for k in ['price_under_cents','price_over_cents']))
d['ResultMode']=enum('products','variants')
d['LexicalHit']=obj({'product_id':U,'variant_id':U,'score':{'type':'number','minimum':0,'maximum':1e12},'relevance_bucket':I32})
d['LexicalInput']=union(tag('exact_token_and',key='profile'),tag('bm25',key='profile'),tag('bm25_literal',key='profile'),tag('supplied',{'hits':arr(ref('LexicalHit'))},key='profile'))
d['Context']=obj({'category_id':union(U32,{'type':'null'}),'tags':arr(S)},[])
d['Scope']=union(tag('global'),tag('category',{'category_id':U32}),tag('query',{'query':S}))
d['Condition']=union(tag('query_equals',{'query':S}),tag('query_contains_all',{'tokens':arr(S)}),tag('context_category',{'category_id':U32}),tag('context_tag',{'tag':S}))
d['Target']=union(tag('all'),tag('products',{'ids':arr(U)}),tag('variants',{'ids':arr(U)}),tag('filters',{'filters':arr(ref('Filter'))}))
d['Action']=union(tag('hide'),tag('group',{'order':U32,'rank':I32},required=['rank']),tag('pin',{'position':U}))
d['Rule']=obj({'id':S,'scope':ref('Scope'),'priority':I32,'conditions':arr(ref('Condition')),'target':ref('Target'),'action':ref('Action')},['id','target','action'])
d['PersonalSignal']=obj({'target':ref('Target'),'factor':{'type':'number','minimum':.25,'maximum':4}})
d['FacetField']=union(tag('attribute',{'name':S}),*(tag(k) for k in ['brand','category','product_type','price_cents']))
d['FacetRequest']=obj({'name':S,'field':ref('FacetField')})
d['Request']=obj({'q':S,'filters':arr(ref('Filter')),'in_stock_only':B,'result_mode':ref('ResultMode'),'facet_count_mode':union(ref('ResultMode'),{'type':'null'}),'facets':arr(ref('FacetRequest')),'lexical':ref('LexicalInput'),'context':ref('Context'),'rules':arr(ref('Rule')),'personal_signals':arr(ref('PersonalSignal')),'sort':enum('relevance','price_asc','price_desc'),'sort_policy':enum('shopper_overrides','merch_overrides'),'offset':U,'limit':U},[])
d['VariantInput']=obj({'variant_id':U,'price_cents':{'type':'integer','minimum':0,'maximum':2**63-1},'available_units':U32,'in_stock':B,'attributes':ref('Attributes')},['variant_id','price_cents','available_units','in_stock'])
d['ProductInput']=obj({'product_id':U,'title':S,'brand_id':U32,'category_id':U32,'product_type_id':U32,'attributes':ref('Attributes'),'variants':arr(ref('VariantInput'))},['product_id','title','brand_id','category_id','product_type_id','variants'])
d['CatalogInput']=obj({'products':arr(ref('ProductInput'))})
d['ReplayInput']=obj({'catalog':ref('CatalogInput'),'requests':arr(ref('Request'))})
# Public schema type identifiers; filenames are derived, not credential pairs.
for kind in ['Request', 'CatalogInput', 'ReplayInput']:
    name = kind.removesuffix('Input').lower()
    value={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:athos80-experimental-v1.1:'+name,'title':kind,'description':'Experimental input shape. Rust runtime additionally validates identity uniqueness, finite values, active pin positions, and contextual semantics; HTTP adds documented resource caps. No Athos compatibility claim.','$defs':d,**ref(kind)}
    (root/(name+'.schema.json')).write_text(json.dumps(value,indent=2)+'\n')
