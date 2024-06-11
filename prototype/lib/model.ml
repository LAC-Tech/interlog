let ( << ) f g x = f (g x)
let ( >> ) f g x = g (f x)

module Addr : sig
  type t [@@deriving show, eq, hash]
  val create : unit -> t
end = struct
  type t = int [@@deriving show, eq, hash]
  let create () = Random.full_int Int.max_int
end

module Addrtbl = Hashtbl.Make (Addr)

module Event = struct
  type id = { origin : Addr.t; pos : int }
  type payload = string
  type t = { id : id; payload : payload }
end

type msg = 
  | SyncRes of Event.t list

(* Maps the remote log position to the local one *)
module RemoteToLocal : sig
  type map
  type pos = { remote : int; local : int}
  val create_map : Event.id option -> map
  val add : map -> Event.t -> unit
end = struct
  type map = int Dynarray.t
  type pos = { remote : int; local : int}
  let create_map = function
    | Some initial -> Dynarray.of_array [|initial.Event.pos|]
    | None -> Dynarray.create ()
  let add rlpm (e: Event.t) =
    (* Events must be added sequentially, without gaps *)
    let expected = Dynarray.length rlpm in
    if e.id.pos = expected then 
      ()(* Dynarray.add_last rlpm pos.local *)
    else 
      ()(*Printf.eprintf "given remote pos %d, expecting %d" pos.remote expected
      *)
end

(* Not only performs key lookup, but also a version vector *)
module Index : sig
  type t
  val create : unit -> t
  val add_addrs : t -> Addr.t list -> unit
  val add_ids : t -> Event.id list -> unit
end = struct
  type t = RemoteToLocal.map Addrtbl.t

  let create () = Addrtbl.create 128

  let add_addrs index =
    let add_addr addr =
      if not (Addrtbl.mem index addr) then
        Addrtbl.add index addr (RemoteToLocal.create_map None)
    in
    List.iter add_addr

  type validation_err =
    | NonMotonicPos of Addr.t

  let rec is_continuous = function
  | x :: y :: rest -> (y = x + 1) && is_continuous (y :: rest)
  | _ -> true

  let validate_ids ids =
    let result = 
      ids |> List.to_seq |> Seq.group (fun a b -> a.Event.origin = b.Event.origin)
    in
    0

  let add_ids index =
    let add_id batch_elem =
      (*
      match AddrHashtbl.find_opt index id.Event.origin with
      | Some offsets -> RemoteToLocal.add offsets { remote = id.Event.pos; local =
        local_pos }
      | None -> 
        let initial = RemoteToLocal.create_map (Some id) in
        AddrHashtbl.add index id.origin initial 
      *)
      ()
    in
    List.iter add_id
end

module Actor : sig
  type t

  val create : Addr.t -> t
  val add_acquaintances : t -> Addr.t list -> unit
  val addr : t -> Addr.t
  val recv : t -> msg -> unit
end = struct
  type t = { addr : Addr.t; log : Event.t Dynarray.t; index : Index.t }

  let create addr = { addr; log = Dynarray.create (); index = Index.create () }
  let add_acquaintances actor = Index.add_addrs actor.index
  let addr actor = actor.addr

  let recv actor = function
    | SyncRes events ->
        let ids = events |> List.map (fun e -> e.Event.id) in
        Index.add_ids actor.index ids;
        Dynarray.append_list actor.log events
end
