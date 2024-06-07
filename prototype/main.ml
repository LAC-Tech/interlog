type addr = int;;
type event = string;;
type msg =
  | Append of event array

module Actor : sig
  type t
  val create : addr -> t
  val recv : t -> msg -> unit
end = struct
  type t = { log: event Dynarray.t }
  let create addr = { log = Dynarray.create () }
  let recv actor = function
    | Append events -> Dynarray.append_array actor.log events
end

let actors = Hashtbl.create 128;;

module Network : sig
  val send : msg -> addr -> unit
  val spawn : unit -> unit
end = struct
  let send msg addr = match Hashtbl.find_opt actors addr with
  | Some a -> Actor.recv a msg
  | None -> Printf.eprintf "no such address %d" addr
  let spawn () = 
    let addr = Random.full_int (Int.max_int - 1) in
    if Hashtbl.mem actors addr then
      raise (Invalid_argument (Printf.sprintf "duplicate address %d" addr))
    else
      Hashtbl.add actors addr (Actor.create addr)


end

let rng = Random.self_init ();;
