open Model

module Network : sig
  val send : msg -> Addr.t -> unit
  val spawn : Addr.t array -> Actor.t
  val find : Addr.t -> Actor.t
end = struct
  let actors : Actor.t Addrtbl.t = Addrtbl.create 128

  let send msg addr =
    match Addrtbl.find_opt actors addr with
    | Some a -> Actor.recv a msg
    | None -> Printf.eprintf "no such address %s" Addr.(show addr)

  let spawn acquaintances =
    let addr = Addr.create () in
    if Addrtbl.mem actors addr then
      raise
        (Invalid_argument
           (Printf.sprintf "duplicate address %s" Addr.(show addr)))
    else
      let actor = Actor.create addr in
      Actor.add_acquaintances actor acquaintances;
      Addrtbl.add actors addr actor;
      actor

  let find = Addrtbl.find actors
end

let () =
  let seed = int_of_float (Unix.time ()) in
  Random.init seed;
  Printf.printf "Seed: %d\n" seed;
  let a = Network.spawn [||] in
  let b = Network.spawn [|Actor.addr a|] in
  ()
