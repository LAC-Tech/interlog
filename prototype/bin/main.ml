let ( << ) f g x = f (g x)
let ( >> ) f g x = g (f x)

module Addr : sig
  type t [@@deriving show, eq, hash]

  val create : unit -> t
end = struct
  type t = int [@@deriving show, eq, hash]

  let create () = Random.full_int Int.max_int
end

module AddrHashtbl = Hashtbl.Make (Addr)

module Event = struct
  type id = { origin : Addr.t; pos : int }
  type payload = string
  type t = { id : id; payload : payload }
  type batch_elem = { start_origin_pos: int; payload: payload array }
  type batch = batch_elem AddrHashtbl.t

end

type msg = 
  | SyncRes of Event.batch

(* Maps the remote log position to the local one *)
module RemoteToLocal : sig
  type map
  type pos = { remote : int; local : int}
  val create_map : Event.id option -> map
  val add : map -> Event.batch_elem -> unit
end = struct
  type map = int Dynarray.t
  type pos = { remote : int; local : int}
  let create_map = function
    | Some initial -> Dynarray.of_array [|initial.Event.pos|]
    | None -> Dynarray.create ()
  let add rlpm (batch_elem: Event.batch_elem) =
    (* Events must be added sequentially, without gaps *)
    let expected = Dynarray.length rlpm in
    if batch_elem.start_origin_pos = expected then 
      Dynarray.add_last rlpm pos.local
    else 
      Printf.eprintf "given remote pos %d, expecting %d" pos.remote expected

end

(* Not only performs key lookup, but also a version vector *)
module Index : sig
  type t

  val create : unit -> t
  val add_addrs : t -> Addr.t Seq.t -> unit
  val add_batch : t -> Event.batch -> unit
end = struct
  type t = RemoteToLocal.map AddrHashtbl.t

  let create () = AddrHashtbl.create 128

  let add_addrs index =
    let add_addr addr =
      if not (AddrHashtbl.mem index addr) then
        AddrHashtbl.add index addr (RemoteToLocal.create_map None)
    in
    Seq.iter add_addr

  let add_batch index =
    let add_id batch_elem =
      match AddrHashtbl.find_opt index id.Event.origin with
      | Some offsets -> RemoteToLocal.add offsets { remote = id.Event.pos; local =
        local_pos }
      | None -> 
        let initial = RemoteToLocal.create_map (Some id) in
        AddrHashtbl.add index id.origin initial 
    in
    Seq.iter add_id
end

module Actor : sig
  type t

  val create : Addr.t -> t
  val add_acquaintances : t -> Addr.t array -> unit
  val addr : t -> Addr.t
  val recv : t -> msg -> unit
end = struct
  type t = { addr : Addr.t; log : Event.t Dynarray.t; index : Index.t }

  let create addr = { addr; log = Dynarray.create (); index = Index.create () }
  let add_acquaintances actor = Array.to_seq >> Index.add_addrs actor.index
  let addr actor = actor.addr

  let recv actor = function
    | SyncRes events ->
        let ids = events |> Array.to_seq |> Seq.map (fun e -> e.Event.id) in
        Index.add_ids actor.index ids;
        Dynarray.append_array actor.log events
end

module Network : sig
  val send : msg -> Addr.t -> unit
  val spawn : Addr.t array -> Actor.t
  val find : Addr.t -> Actor.t
end = struct
  let actors : Actor.t AddrHashtbl.t = AddrHashtbl.create 128

  let send msg addr =
    match AddrHashtbl.find_opt actors addr with
    | Some a -> Actor.recv a msg
    | None -> Printf.eprintf "no such address %s" Addr.(show addr)

  let spawn acquaintances =
    let addr = Addr.create () in
    if AddrHashtbl.mem actors addr then
      raise
        (Invalid_argument
           (Printf.sprintf "duplicate address %s" Addr.(show addr)))
    else
      let actor = Actor.create addr in
      Actor.add_acquaintances actor acquaintances;
      AddrHashtbl.add actors addr actor;
      actor

  let find = AddrHashtbl.find actors
end

let () =
  let seed = int_of_float (Unix.time ()) in
  Random.init seed;
  Printf.printf "Seed: %d\n" seed;
  let a = Network.spawn [||] in
  let b = Network.spawn [|Actor.addr a|] in
  ()
