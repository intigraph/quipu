//! Bind ingress profiles to the exact shapes and prospective focus graph.
//! Shape-declared units cannot be removed by a per-call profile. sh:group is
//! never read. No shape, registry or graph is modified by this decoder.
use std::collections::{BTreeMap,BTreeSet};
use oxrdf::{Term,Triple};
use oxrdfio::{RdfFormat,RdfParser};
use crate::error::{Error,Result};
use crate::merge3::{FocusView,TripleView};
use crate::merge_lists::{Mode as ListMode,Slot};
use crate::merge_session::{ListBinding,Profile};
use crate::share_merge::Graph;

const SH:&str="http://www.w3.org/ns/shacl#";
const RDF:&str="http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const Q:[&str;2]=["http://quipu.dev/ontology/","http://quipu.dev/ns#"];
fn invalid(message:&str)->Error {Error::InvalidValue(format!("merge profile: {message}"))}
fn objects<'a>(graph:&'a Graph,subject:&str,predicate:&str)->Vec<&'a Term> {
    graph.iter().filter(|t|t.subject.to_string()==subject && t.predicate.as_str()==predicate).map(|t|&t.object).collect()
}
fn annotated<'a>(graph:&'a Graph,subject:&str,name:&str)->Vec<&'a Term> {
    Q.iter().flat_map(|ns|objects(graph,subject,&format!("{ns}{name}"))).collect()
}
fn iri(term:&Term)->Result<String> {match term {Term::NamedNode(n)=>Ok(n.as_str().into()),_=>Err(invalid("simple IRI path required"))}}

fn focuses(shapes:&Graph,shape:&str,view:&TripleView,subjects:&BTreeSet<String>)->Result<BTreeSet<String>> {
    let mut result=BTreeSet::new();
    for target in objects(shapes,shape,&format!("{SH}targetNode")) {result.insert(target.to_string());}
    for subject in subjects {
        for class in objects(shapes,shape,&format!("{SH}targetClass")) {
            if view.instance_of(subject,&iri(class)?)? {result.insert(subject.clone());}
        }
        for path in objects(shapes,shape,&format!("{SH}targetSubjectsOf")) {
            if view.has_out(subject,&iri(path)?)? {result.insert(subject.clone());}
        }
        for path in objects(shapes,shape,&format!("{SH}targetObjectsOf")) {
            if view.has_in(subject,&iri(path)?)? {result.insert(subject.clone());}
        }
    }
    Ok(result)
}

fn paths(graph:&Graph,head:&Term)->Result<Vec<String>> {
    let mut head=head.to_string();let nil=format!("<{RDF}nil>");
    let mut seen=BTreeSet::new();let mut result=Vec::new();
    while head!=nil {
        if seen.len()>=128 || !seen.insert(head.clone()) {return Err(invalid("cyclic/oversized unit paths"));}
        let first=objects(graph,&head,&format!("{RDF}first"));let rest=objects(graph,&head,&format!("{RDF}rest"));
        if first.len()!=1 || rest.len()!=1 {return Err(invalid("unit paths require one first/rest"));}
        let path=iri(first[0])?;
        if result.contains(&path) {return Err(invalid("duplicate unit path"));}
        result.push(path);head=rest[0].to_string();
    }
    if result.is_empty() {return Err(invalid("empty merge unit"));}
    Ok(result)
}

pub(crate) fn bind(mut profile:Profile,shapes:&str,graphs:[&Graph;3])->Result<Profile> {
    if profile.schema!="quipu-merge-profile/v1" {return Err(invalid("unknown schema"));}
    let digest=crate::share::sha256(shapes.as_bytes());
    if profile.shapes_hash.as_ref().is_some_and(|old|old!=&digest) {return Err(invalid("shape bytes changed"));}
    profile.shapes_hash=Some(digest);
    if profile.lists.len()>4096 || profile.units.len()>4096 {return Err(invalid("profile budget exceeded"));}
    let mut shapes_graph=Graph::new();
    if !shapes.trim().is_empty() {
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(shapes.as_bytes()) {
            if shapes_graph.len()>=100_000 {return Err(invalid("shape budget exceeded"));}
            shapes_graph.insert(quad.map(Triple::from).map_err(|e|invalid(&e.to_string()))?);
        }
    }
    let view=TripleView::new(graphs.iter().flat_map(|g|g.iter()));
    let subjects:BTreeSet<String>=graphs.iter().flat_map(|g|g.iter()).flat_map(|t|[t.subject.to_string(),t.object.to_string()]).collect();
    let mut lists:BTreeMap<Slot,ListMode>=BTreeMap::new();
    for list in profile.lists {
        if lists.insert(list.slot,list.mode).is_some() {return Err(invalid("duplicate list binding"));}
    }
    let nodes:BTreeSet<_>=shapes_graph.iter().map(|t|t.subject.to_string()).collect();
    for shape in nodes {
        let unit_values=annotated(&shapes_graph,&shape,"mergeUnit");
        let list_mode=annotated(&shapes_graph,&shape,"listMerge");
        if !unit_values.is_empty() {
            let targets=focuses(&shapes_graph,&shape,&view,&subjects)?;
            for unit in unit_values {
                let declared=annotated(&shapes_graph,&unit.to_string(),"paths");
                if declared.len()>1 {return Err(invalid("multiple unit path lists"));}
                let paths=paths(&shapes_graph,declared.first().copied().unwrap_or(unit))?;
                for subject in &targets {
                    profile.units.push(paths.iter().map(|path|(subject.clone(),path.clone())).collect());
                }
            }
        }
        if !list_mode.is_empty() {
            if list_mode.len()!=1 {return Err(invalid("multiple list modes"));}
            let mode=match list_mode[0] {
                Term::Literal(l) if l.language().is_none() && l.value()=="whole"=>ListMode::Whole,
                Term::Literal(l) if l.language().is_none() && l.value()=="sequence"=>ListMode::Sequence,
                _=>return Err(invalid("listMerge must be whole or sequence")),
            };
            let path=objects(&shapes_graph,&shape,&format!("{SH}path"));
            if path.len()!=1 {return Err(invalid("list property needs one simple path"));}
            let path=iri(path[0])?;
            let mut targets=focuses(&shapes_graph,&shape,&view,&subjects)?;
            for parent in shapes_graph.iter().filter(|t|t.predicate.as_str()==format!("{SH}property") && t.object.to_string()==shape) {
                targets.extend(focuses(&shapes_graph,&parent.subject.to_string(),&view,&subjects)?);
            }
            for subject in targets {
                let key=(subject,path.clone());
                if lists.get(&key).is_some_and(|old|*old!=mode) {return Err(invalid("call profile conflicts with shape list mode"));}
                lists.insert(key,mode);
            }
        }
    }
    profile.lists=lists.into_iter().map(|(slot,mode)|ListBinding{slot,mode}).collect();
    profile.units.sort();profile.units.dedup();
    for unit in &profile.units {
        if unit.is_empty() || unit.len()>128 {return Err(invalid("empty/oversized unit"));}
        for (subject,path) in unit {
            if !subjects.contains(subject) || oxrdf::NamedNode::new(path.clone()).is_err() {return Err(invalid("unknown unit subject or invalid path"));}
        }
    }
    Ok(profile)
}
