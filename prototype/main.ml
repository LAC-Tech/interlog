type addr = int;;
type offset = int;;

module Event = struct
  type id = {origin: addr; pos: int}
  type payload = string
  type t = { id: id; payload: payload }
end

type msg =
  | Sync of {events: Event.t array; from: addr }

module Index : sig
  type t
  val create : unit -> t
  val add_seq : t -> Event.id Seq.t -> unit
end = struct
  type t = (addr, offset Dynarray.t) Hashtbl.t 

  let create () = Hashtbl.create 128

  let add_seq index ids = 
    let add_id (id: Event.id) = match Hashtbl.find_opt index id.origin with
    | Some offsets -> Dynarray.add_last offsets id.pos
    | None -> Hashtbl.add index id.origin (Dynarray.of_array [|id.pos|])
    in
    Seq.iter add_id ids
end

module Actor : sig
  type t
  val create : addr -> t
  val recv : t -> msg -> unit
end = struct
  type t = { log: Event.t Dynarray.t; index: Index.t }

  let create addr = { log = Dynarray.create (); index = Index.create () } 

  let recv actor = function
    | Sync {events; from} -> 
        let ids = 
          events |> Array.to_seq |> Seq.map (fun (e: Event.t) -> e.id)
        in
        Index.add_seq actor.index ids;
        Dynarray.append_array actor.log events
end


module Network : sig
  val send : msg -> addr -> unit
  val spawn : unit -> addr
  val inspect : addr -> Actor.t
end = struct
  let actors = Hashtbl.create 128

  let send msg addr = match Hashtbl.find_opt actors addr with
  | Some a -> Actor.recv a msg
  | None -> Printf.eprintf "no such address %d" addr

  let spawn () = 
    let addr = Random.full_int Int.max_int in
    if Hashtbl.mem actors addr then
      raise (Invalid_argument (Printf.sprintf "duplicate address %d" addr))
    else (
      Hashtbl.add actors addr (Actor.create addr);
      addr
    )

  let inspect = Hashtbl.find actors


end

let rng = Random.self_init ();;
