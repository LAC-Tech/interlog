type addr = int
type offset = int

let (<<) f g x = f(g(x))
let (>>) f g x = g(f(x))

module Event = struct
  type id = { origin : addr; pos : int }
  type payload = string
  type t = { id : id; payload : payload }
end

type msg = Sync of Event.t array

module Index : sig
  type t

  val create : unit -> t
  val add_addrs : t -> addr Seq.t -> unit
  val add_ids : t -> Event.id Seq.t -> unit
end = struct
  type t = (addr, offset Dynarray.t) Hashtbl.t

  let create () = Hashtbl.create 128

  let add_addrs index =
    let add_addr addr =
      if not (Hashtbl.mem index addr) then
        Hashtbl.add index addr (Dynarray.create ())
    in
    Seq.iter add_addr

  let add_ids index =
    let add_id (id : Event.id) =
      match Hashtbl.find_opt index id.origin with
      | Some offsets -> Dynarray.add_last offsets id.pos
      | None -> Hashtbl.add index id.origin (Dynarray.of_array [| id.pos |])
    in
    Seq.iter add_id
end

module Actor : sig
  type t
  val create : addr -> t
  val add_acquaintances : t -> addr array -> unit
  val addr : t -> addr
  val recv : t -> msg -> unit
end = struct
  type t = { addr : addr; log : Event.t Dynarray.t; index : Index.t }

  let create addr = { addr; log = Dynarray.create (); index = Index.create () }
  let add_acquaintances actor = Array.to_seq >> Index.add_addrs actor.index
  let addr actor = actor.addr

  let recv actor = function
    | Sync events ->
        let ids = events |> Array.to_seq |> Seq.map (fun e -> e.Event.id) in
        Index.add_ids actor.index ids;
        Dynarray.append_array actor.log events
end

module Network : sig
  val send : msg -> addr -> unit
  val spawn : unit -> Actor.t
  val find : addr -> Actor.t
end = struct
  let actors = Hashtbl.create 128

  let send msg addr =
    match Hashtbl.find_opt actors addr with
    | Some a -> Actor.recv a msg
    | None -> Printf.eprintf "no such address %d" addr

  let spawn () =
    let addr = Random.full_int Int.max_int in
    if Hashtbl.mem actors addr then
      raise (Invalid_argument (Printf.sprintf "duplicate address %d" addr))
    else
      let actor = Actor.create addr in
      Hashtbl.add actors addr actor;
      actor

  let find = Hashtbl.find actors
end

let () =
  let seed = int_of_float (Unix.time ()) in
  Random.init seed;
  Printf.printf "Seed: %d\n" seed
  let a = Network.spawn ();;
  let b = Network.spawn ();;
  Actor.add_acquaintances b [|Actor.addr a|]
